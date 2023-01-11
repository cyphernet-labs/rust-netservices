//! The module provides adaptors for networking sessions (see [`NetSession`]
//! trait) to become a [`reactor`]-managed [`Resource`]es, which can be polled
//! for I/O events in a non-blocking but synchronous mode in a dedicated reactor
//! thread.
//!
//! The module  allows to solve [C10k] problem with multiple connections by
//! utilizing non-blocking `poll` sys-calls. It uses the same principle as
//! async runtimes (`tokio` and others), but provides much simpler API and can
//! run without heap of dependencies introduced by async runtimes.
//!
//! [C10k]: https://en.wikipedia.org/wiki/C10k_problem

use std::collections::VecDeque;
use std::fmt::{Debug, Display, Formatter};
use std::io::Write;
use std::net::{TcpListener, ToSocketAddrs};
use std::os::unix::io::{AsRawFd, RawFd};
use std::time::Duration;
use std::{io, net};

use reactor::poller::IoType;
use reactor::{Io, Resource, WriteAtomic, WriteError};

use crate::tunnel::READ_BUFFER_SIZE;
use crate::{NetConnection, NetListener, NetSession, Proxy};

// TODO: Make these parameters configurable
/// Socket read buffer size.
const HEAP_BUFFER_SIZE: usize = u16::MAX as usize;
/// Maximum time to wait when reading from a socket.
const READ_TIMEOUT: Duration = Duration::from_secs(6);
/// Maximum time to wait when writing to a socket.
const WRITE_TIMEOUT: Duration = Duration::from_secs(3);

/// An event happening for a [`NetAccept`] network listener and delivered to a
/// [`reactor::Handler`].
#[derive(Debug)]
pub enum ListenerEvent<S: NetSession> {
    /// A new incoming connection was accepted.
    ///
    /// The connection is already upgraded to a [`NetSession`], however no I/O
    /// events was read or wrote for it yet.
    Accepted(S),

    /// Listener `accept` call has resulted in a I/O error from the OS.
    Failure(io::Error),
}

/// A reactor-manageable network listener (TCP, but not limiting to) which can
/// be aware of additional encryption, authentication and other forms of
/// transport-layer protocols which will be automatically injected into accepted
/// connections.
#[derive(Debug)]
pub struct NetAccept<S: NetSession, L: NetListener<Stream = S::Connection> = TcpListener> {
    /// The `session_context` object provides information for encryption,
    /// authentication and other protocols which are a part of the application-
    /// specific transport layer and are automatically injected into the
    /// new sessions constructed by this listener before they are inserted into
    /// the [`reactor`] and notifications are delivered to [`reactor::Handler`].
    session_context: S::Context,
    listener: L,
}

impl<L: NetListener<Stream = S::Connection>, S: NetSession> AsRawFd for NetAccept<S, L> {
    fn as_raw_fd(&self) -> RawFd {
        self.listener.as_raw_fd()
    }
}

impl<L: NetListener<Stream = S::Connection>, S: NetSession> io::Write for NetAccept<S, L> {
    fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
        Err(io::ErrorKind::InvalidInput.into())
    }

    fn flush(&mut self) -> io::Result<()> {
        Err(io::ErrorKind::InvalidInput.into())
    }
}

impl<L: NetListener<Stream = S::Connection>, S: NetSession> WriteAtomic for NetAccept<S, L> {
    fn is_ready_to_write(&self) -> bool {
        false
    }

    fn empty_write_buf(&mut self) -> io::Result<bool> {
        Ok(true)
    }

    fn write_or_buf(&mut self, _: &[u8]) -> io::Result<()> {
        Err(io::ErrorKind::InvalidInput.into())
    }
}

impl<L: NetListener<Stream = S::Connection>, S: NetSession> NetAccept<S, L> {
    /// Binds listener to the provided socket address(es) with a given context.
    ///
    /// The `session_context` object provides information for encryption,
    /// authentication and other protocols which are a part of the application-
    /// specific transport layer and are automatically injected into the
    /// new sessions constructed by this listener before they are inserted into
    /// the [`reactor`] and notifications are delivered to [`reactor::Handler`].
    /// The injection is made by calling [`NetSession::accept`] method.
    pub fn bind(addr: &impl ToSocketAddrs, session_context: S::Context) -> io::Result<Self> {
        let listener = L::bind(addr)?;
        listener.set_nonblocking(true)?;
        Ok(Self {
            session_context,
            listener,
        })
    }

    /// Returns the local [`net::SocketAddr`] on which listener accepts
    /// connections.
    pub fn local_addr(&self) -> net::SocketAddr {
        self.listener.local_addr()
    }

    fn handle_accept(&mut self) -> io::Result<S> {
        let mut stream = self.listener.accept()?;
        stream.set_read_timeout(Some(READ_TIMEOUT))?;
        stream.set_write_timeout(Some(WRITE_TIMEOUT))?;
        stream.set_nonblocking(true)?;
        S::accept(stream, self.session_context.clone())
    }
}

impl<L: NetListener<Stream = S::Connection>, S: NetSession> Resource for NetAccept<S, L> {
    type Id = net::SocketAddr;
    type Event = ListenerEvent<S>;

    fn id(&self) -> Self::Id {
        self.listener.local_addr()
    }

    fn interests(&self) -> IoType {
        IoType::read_only()
    }

    fn handle_io(&mut self, io: Io) -> Option<Self::Event> {
        match io {
            Io::Read => Some(match self.handle_accept() {
                Err(err) => ListenerEvent::Failure(err),
                Ok(session) => ListenerEvent::Accepted(session),
            }),
            Io::Write => None,
        }
    }

    fn disconnect(self) -> io::Result<()> {
        // We disconnect by dropping the self
        Ok(())
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub enum LinkDirection {
    Inbound,
    Outbound,
}

/// An event happening for a [`NetTransport`] network transport and delivered to
/// a [`reactor::Handler`].
pub enum SessionEvent<S: NetSession> {
    Established(S::Id),
    Data(Vec<u8>),
    Terminated(io::Error),
}

/// A state of [`NetTransport`] network transport.
#[derive(Clone, Copy, Ord, PartialOrd, Eq, PartialEq, Hash, Debug)]
pub enum TransportState {
    /// The transport is initiated, but the connection has not established yet.
    /// This happens only for outgoing connections due to the use of
    /// non-blocking version of a `connect` sys-call. The state is switched once
    /// we receive first notification on a `write` event on this resource from
    /// the reactor `poll`.
    Init,

    /// The connection is established, but the session handshake is still in
    /// progress. This happens while encryption handshake, authentication and
    /// other protocols injected into the session haven't completed yet.
    Handshake,

    /// The session is active; all handshakes had completed.
    Active,

    /// Session was terminated by any reason: local shutdown, remote orderly
    /// shutdown, connectivity issue, dropped connections, encryption or
    /// authentication problem etc. Reading and writing from the resource in
    /// this state will result in an error ([`io::Error`]).
    Terminated,
}

/// Net transport is an adaptor around specific [`NetSession`] (implementing
/// session management, including optional handshake, encoding etc) to be used
/// as a transport resource in a [`reactor::Reactor`].
#[derive(Debug)]
pub struct NetTransport<S: NetSession> {
    state: TransportState,
    session: S,
    link_direction: LinkDirection,
    write_intent: bool,
    read_buffer: Box<[u8; HEAP_BUFFER_SIZE]>,
    write_buffer: VecDeque<u8>,
}

impl<S: NetSession> Display for NetTransport<S> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        if let Some(addr) = self.session.peer_addr() {
            Display::fmt(&addr, f)
        } else {
            Display::fmt(&self.session.transient_addr(), f)
        }
    }
}

impl<S: NetSession> AsRawFd for NetTransport<S> {
    fn as_raw_fd(&self) -> RawFd {
        self.session.as_raw_fd()
    }
}

impl<S: NetSession> NetTransport<S> {
    pub fn accept(session: S) -> io::Result<Self> {
        Self::with_state(session, TransportState::Handshake, LinkDirection::Inbound)
    }

    #[cfg(feature = "socket2")]
    pub fn connect<P: Proxy>(
        addr: S::PeerAddr,
        context: S::Context,
        proxy: &P,
    ) -> Result<Self, P::Error> {
        let session = S::connect_nonblocking(addr, context, proxy)?;
        Self::with_state(session, TransportState::Init, LinkDirection::Outbound)
            .map_err(P::Error::from)
    }

    /*
    pub fn connect_blocking<P: Proxy>(
        addr: Self::PeerAddr,
        context: &Self::Context,
        proxy: &P,
    ) -> Result<Self, P::Error> {
        let session = S::connect_blocking(addr, context, proxy)?;
        Self::with_state(session, TransportState::Handshake, LinkDirection::Outbound)
            .map_err(P::Error::from)
    }
     */

    /// Constructs reactor-managed resource around an existing [`NetSession`].
    ///
    /// NB: Must not be called for connections created in a non-blocking mode!
    ///
    /// # Errors
    ///
    /// If a session can be put into a non-blocking mode.
    pub fn with_session(mut session: S, link_direction: LinkDirection) -> io::Result<Self> {
        let state = if session.is_established() {
            // If we are disconnected, we will get instantly updated from the
            // reactor and the state will change automatically
            TransportState::Active
        } else {
            TransportState::Handshake
        };
        session.set_nonblocking(true)?;
        Ok(Self {
            state,
            session,
            link_direction,
            write_intent: false,
            read_buffer: Box::new([0u8; READ_BUFFER_SIZE]),
            write_buffer: empty!(),
        })
    }

    /// Tries to unwrap the underlying [`NetSession`] object.
    ///
    /// Attempts to empty data buffered for a write operation in a non-blocking
    /// way. If the non-blocking emptying is not possible errors with
    /// [`io::ErrorKind::WouldBlock`] kind of [`io::Error`].
    ///
    /// # Errors
    ///
    /// Returns the consumed `self` as a part of an error.
    ///
    /// If the write buffer can't be emptied in a non-blocking way errors with
    /// [`io::ErrorKind::WouldBlock`] kind of [`io::Error`].
    ///
    /// If the connection is failed and the write buffer has some data, errors
    /// with the connection failure code.
    pub fn into_session(mut self) -> Result<S, (Self, io::Error)> {
        if let Err(err) = self.empty_write_buf() {
            return Err((self, err));
        }
        Ok(self.session)
    }

    fn with_state(
        mut session: S,
        state: TransportState,
        link_direction: LinkDirection,
    ) -> io::Result<Self> {
        session.set_read_timeout(Some(READ_TIMEOUT))?;
        session.set_write_timeout(Some(WRITE_TIMEOUT))?;
        Ok(Self {
            state,
            session,
            link_direction,
            write_intent: false,
            read_buffer: Box::new([0u8; READ_BUFFER_SIZE]),
            write_buffer: empty!(),
        })
    }

    pub fn state(&self) -> TransportState {
        self.state
    }
    pub fn is_active(&self) -> bool {
        self.state == TransportState::Active
    }

    pub fn is_inbound(&self) -> bool {
        self.link_direction() == LinkDirection::Inbound
    }
    pub fn is_outbound(&self) -> bool {
        self.link_direction() == LinkDirection::Outbound
    }
    pub fn link_direction(&self) -> LinkDirection {
        self.link_direction
    }

    pub fn local_addr(&self) -> <S::Connection as NetConnection>::Addr {
        self.session.local_addr()
    }

    pub fn remote_addr(&self) -> Option<S::PeerAddr> {
        self.session.peer_addr()
    }

    pub fn transient_addr(&self) -> S::TransientAddr {
        self.session.transient_addr()
    }

    pub fn peer_id(&self) -> Option<S::Id> {
        self.session.session_id()
    }

    pub fn expect_peer_id(&self) -> S::Id {
        self.session.expect_id()
    }

    pub fn write_buf_len(&self) -> usize {
        self.write_buffer.len()
    }

    fn terminate(&mut self, reason: io::Error) -> SessionEvent<S> {
        #[cfg(feature = "log")]
        log::trace!(target: "transport", "Terminating connection {self} due to {reason:?}");

        self.state = TransportState::Terminated;
        SessionEvent::Terminated(reason)
    }

    fn handle_writable(&mut self) -> Option<SessionEvent<S>> {
        if !self.session.is_established() {
            let _ = self.session.write(&[]);
            self.write_intent = true;
            return None;
        }
        match self.flush() {
            Ok(_) => {
                self.write_intent = false;
                None
            }
            // In this case, the write couldn't complete. Leave `needs_flush` set
            // to be notified when the socket is ready to write again.
            Err(err)
                if [io::ErrorKind::WouldBlock, io::ErrorKind::WriteZero].contains(&err.kind()) =>
            {
                self.write_intent = true;
                None
            }
            Err(err) => Some(self.terminate(err)),
        }
    }

    fn handle_readable(&mut self) -> Option<SessionEvent<S>> {
        // Nb. Since `poll`, which this reactor is based on, is *level-triggered*,
        // we will be notified again if there is still data to be read on the socket.
        // Hence, there is no use in putting this socket read in a loop, as the second
        // invocation would likely block.
        match self.session.read(self.read_buffer.as_mut()) {
            Ok(0) if !self.session.is_established() => None,
            Ok(0) => Some(SessionEvent::Terminated(
                io::ErrorKind::ConnectionReset.into(),
            )),
            Ok(len) => Some(SessionEvent::Data(self.read_buffer[..len].to_vec())),
            Err(err) if err.kind() == io::ErrorKind::WouldBlock => {
                // This shouldn't normally happen, since this function is only called
                // when there's data on the socket. We leave it here in case external
                // conditions change.
                log::warn!(target: "transport",
                    "WOULD_BLOCK on resource which had read intent - probably normal thing to happen"
                );
                None
            }
            Err(err) => Some(self.terminate(err)),
        }
    }
}

impl<S: NetSession> Resource for NetTransport<S> {
    // TODO: Use S::SessionId instead
    type Id = RawFd;
    type Event = SessionEvent<S>;

    fn id(&self) -> Self::Id {
        self.session.as_raw_fd()
    }

    fn interests(&self) -> IoType {
        match self.state {
            TransportState::Init => IoType::write_only(),
            TransportState::Terminated => IoType::none(),
            TransportState::Active | TransportState::Handshake if self.write_intent => {
                IoType::read_write()
            }
            TransportState::Active | TransportState::Handshake => IoType::read_only(),
        }
    }

    fn handle_io(&mut self, io: Io) -> Option<Self::Event> {
        debug_assert_ne!(
            self.state,
            TransportState::Terminated,
            "I/O on terminated transport"
        );

        let mut force_write_intent = false;
        if self.state == TransportState::Init {
            #[cfg(feature = "log")]
            log::debug!(target: "transport", "Transport {self} is connected, initializing handshake");

            force_write_intent = true;
            self.state = TransportState::Handshake;
        } else if self.state == TransportState::Handshake {
            debug_assert!(!self.session.is_established());
            #[cfg(feature = "log")]
            log::trace!(target: "transport", "Transport {self} got I/O while in handshake mode");
        }

        let resp = match io {
            Io::Read => self.handle_readable(),
            Io::Write => self.handle_writable(),
        };

        // During handshake, after each read we need to write
        if (io == Io::Read && self.state == TransportState::Handshake) || force_write_intent {
            self.write_intent = true;
        }

        if matches!(&resp, Some(SessionEvent::Terminated(e)) if e.kind() == io::ErrorKind::ConnectionReset)
            && self.state != TransportState::Handshake
        {
            #[cfg(feature = "log")]
            log::debug!(target: "transport", "Peer {self} has reset the connection");

            self.state = TransportState::Terminated;
            resp
        } else if self.session.is_established() && self.state == TransportState::Handshake {
            #[cfg(feature = "log")]
            log::debug!(target: "transport", "Handshake with {self} is complete");

            // We just got connected; may need to send output
            self.write_intent = true;
            self.state = TransportState::Active;
            Some(SessionEvent::Established(self.session.expect_id()))
        } else {
            resp
        }
    }

    fn disconnect(self) -> io::Result<()> {
        self.session.disconnect()
    }
}

impl<S: NetSession> Write for NetTransport<S> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self.write_atomic(buf) {
            Ok(_) => Ok(buf.len()),
            Err(WriteError::NotReady) => Err(io::ErrorKind::NotConnected.into()),
            Err(WriteError::Io(err)) => Err(err),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        self.session.flush()
    }
}

impl<S: NetSession> WriteAtomic for NetTransport<S> {
    fn is_ready_to_write(&self) -> bool {
        self.state == TransportState::Active
    }

    fn empty_write_buf(&mut self) -> io::Result<bool> {
        let len = self.session.write(self.write_buffer.make_contiguous())?;
        self.write_buffer.drain(..len);
        if self.write_buf_len() > 0 {
            return Err(io::ErrorKind::WouldBlock.into());
        }
        Ok(len > 0)
    }

    fn write_or_buf(&mut self, buf: &[u8]) -> io::Result<()> {
        let len = self.session.write(self.write_buffer.make_contiguous())?;
        if len < self.write_buffer.len() {
            self.write_buffer.drain(..len);
            self.write_buffer.extend(buf);
            return Ok(());
        }
        self.write_buffer.drain(..);
        match self.session.write(&buf) {
            Err(err) => Err(err),
            Ok(len) if len < buf.len() => {
                self.write_buffer.extend(&buf[len..]);
                Ok(())
            }
            Ok(_) => Ok(()),
        }
    }
}
