use std::collections::VecDeque;
use std::fmt::{Debug, Display, Formatter};
use std::io::{Read, Write};
use std::net::{TcpListener, ToSocketAddrs};
use std::os::unix::io::{AsRawFd, RawFd};
use std::time::Duration;
use std::{io, net};

use reactor::poller::IoType;
use reactor::{Io, Resource, WriteAtomic, WriteError};

use crate::{NetConnection, NetListener, NetSession};

/// Socket read buffer size.
const READ_BUFFER_SIZE: usize = u16::MAX as usize;
/// Maximum time to wait when reading from a socket.
const READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(6);
/// Maximum time to wait when writing to a socket.
const WRITE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);

#[derive(Debug)]
pub enum ListenerEvent<S: NetSession> {
    Accepted(S),
    Failure(io::Error),
}

#[derive(Debug)]
pub struct NetAccept<S: NetSession, L: NetListener<Stream = S::Connection> = TcpListener> {
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

    fn write_or_buffer(&mut self, _: &[u8]) -> io::Result<()> {
        Err(io::ErrorKind::InvalidInput.into())
    }
}

impl<L: NetListener<Stream = S::Connection>, S: NetSession> NetAccept<S, L> {
    pub fn bind(addr: &impl ToSocketAddrs, session_context: S::Context) -> io::Result<Self> {
        let listener = L::bind(addr)?;
        listener.set_nonblocking(true)?;
        Ok(Self {
            session_context,
            listener,
        })
    }

    pub fn local_addr(&self) -> net::SocketAddr {
        self.listener.local_addr()
    }

    fn handle_accept(&mut self) -> io::Result<S> {
        let mut stream = self.listener.accept()?;
        stream.set_read_timeout(Some(READ_TIMEOUT))?;
        stream.set_write_timeout(Some(WRITE_TIMEOUT))?;
        stream.set_nonblocking(true)?;
        S::accept(stream, &self.session_context)
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

pub enum SessionEvent<S: NetSession> {
    Established(S::Id),
    Data(Vec<u8>),
    Terminated(io::Error),
}

#[derive(Clone, Copy, Ord, PartialOrd, Eq, PartialEq, Hash, Debug)]
pub enum TransportState {
    Init,
    Handshake,
    Active,
    Terminated,
}

/// Net transport is an adaptor around specific [`NetSession`] (implementing
/// session management, including optional handshake, encoding etc) to be used
/// as a transport resource in a [`reactor::Reactor`].
pub struct NetResource<S: NetSession> {
    state: TransportState,
    session: S,
    inbound: bool,
    write_intent: bool,
    read_buffer: Vec<u8>,
    read_buffer_len: usize,
    write_buffer: VecDeque<u8>,
}

impl<S: NetSession> Display for NetResource<S> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        if let Some(addr) = self.session.peer_addr() {
            Display::fmt(&addr, f)
        } else {
            Display::fmt(&self.session.transient_addr(), f)
        }
    }
}

impl<S: NetSession> AsRawFd for NetResource<S> {
    fn as_raw_fd(&self) -> RawFd {
        self.session.as_raw_fd()
    }
}

impl<S: NetSession> NetSession for NetResource<S> {
    type Context = S::Context;
    type Connection = S::Connection;
    type Id = S::Id;
    type PeerAddr = S::PeerAddr;
    type TransientAddr = S::TransientAddr;

    fn accept(connection: Self::Connection, context: &Self::Context) -> io::Result<Self> {
        S::accept(connection, context).and_then(Self::new)
    }

    fn connect_blocking<P: Proxy>(
        addr: Self::PeerAddr,
        context: &Self::Context,
        proxy: &P,
    ) -> Result<Self, P::Error> {
        let session = S::connect_blocking(addr, context, proxy)?;
        Self::with(session, false, TransportState::Handshake).map_err(P::Error::from)
    }

    #[cfg(feature = "socket2")]
    fn connect_nonblocking<P: Proxy>(
        addr: Self::PeerAddr,
        context: &Self::Context,
        proxy: &P,
    ) -> Result<Self, P::Error> {
        let session = S::connect_nonblocking(addr, context, proxy)?;
        Self::with(session, false, TransportState::Init).map_err(P::Error::from)
    }

    fn session_id(&self) -> Option<Self::Id> {
        self.session.session_id()
    }

    fn handshake_completed(&self) -> bool {
        self.session.handshake_completed()
    }

    fn transient_addr(&self) -> Self::TransientAddr {
        self.session.transient_addr()
    }

    fn peer_addr(&self) -> Option<Self::PeerAddr> {
        self.session.peer_addr()
    }

    fn local_addr(&self) -> <Self::Connection as NetConnection>::Addr {
        self.session.local_addr()
    }

    fn read_timeout(&self) -> io::Result<Option<Duration>> {
        self.session.read_timeout()
    }

    fn write_timeout(&self) -> io::Result<Option<Duration>> {
        self.session.write_timeout()
    }

    fn set_read_timeout(&mut self, dur: Option<Duration>) -> io::Result<()> {
        self.session.set_read_timeout(dur)
    }

    fn set_write_timeout(&mut self, dur: Option<Duration>) -> io::Result<()> {
        self.session.set_write_timeout(dur)
    }

    fn set_nonblocking(&mut self, nonblocking: bool) -> io::Result<()> {
        self.session.set_nonblocking(nonblocking)
    }

    fn disconnect(self) -> io::Result<()> {
        self.session.disconnect()
    }
}

impl<S: NetSession> NetResource<S> {
    pub fn new(session: S) -> io::Result<Self> {
        Self::with(session, true, TransportState::Handshake)
    }

    fn with(mut session: S, inbound: bool, state: TransportState) -> io::Result<Self> {
        session.set_read_timeout(Some(READ_TIMEOUT))?;
        session.set_write_timeout(Some(WRITE_TIMEOUT))?;
        Ok(Self {
            state,
            session,
            inbound,
            write_intent: false,
            read_buffer: vec![0; READ_BUFFER_SIZE],
            read_buffer_len: 0,
            write_buffer: VecDeque::new(),
        })
    }

    pub fn is_inbound(&self) -> bool {
        self.inbound
    }

    pub fn is_outbound(&self) -> bool {
        !self.is_inbound()
    }

    pub fn state(&self) -> TransportState {
        self.state
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

    pub fn drain_read_buffer(&mut self) -> Vec<u8> {
        let len = self.read_buffer_len;
        self.read_buffer_len = 0;
        self.read_buffer[..len].to_vec()
    }

    fn terminate(&mut self, reason: io::Error) -> SessionEvent<S> {
        #[cfg(feature = "log")]
        log::trace!(target: "transport", "Terminating connection {self} due to {reason:?}");

        self.state = TransportState::Terminated;
        SessionEvent::Terminated(reason)
    }

    fn handle_writable(&mut self) -> Option<SessionEvent<S>> {
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
        match self
            .session
            .read(&mut self.read_buffer[self.read_buffer_len..])
        {
            Ok(0) if !self.session.handshake_completed() => None,
            Ok(0) => Some(SessionEvent::Terminated(
                io::ErrorKind::ConnectionReset.into(),
            )),
            Ok(len) => {
                self.read_buffer_len += len;
                Some(SessionEvent::Data(self.drain_read_buffer()))
            }
            Err(err) if err.kind() == io::ErrorKind::WouldBlock => {
                debug_assert!(false, "WOULD_BLOCK on resource which had read intent");
                // This shouldn't normally happen, since this function is only called
                // when there's data on the socket. We leave it here in case external
                // conditions change.
                None
            }
            Err(err) => Some(self.terminate(err)),
        }
    }
}

impl<S: NetSession> Resource for NetResource<S> {
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
            debug_assert_eq!(self.read_buffer_len, 0);
            debug_assert!(!self.session.handshake_completed());
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
        } else if self.session.handshake_completed() && self.state == TransportState::Handshake {
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

/// This implementation is used by a reactor and can be used only when the resource
/// is unregistered from the reactor.
// TODO: Consider removing this implementation
impl<S: NetSession> Read for NetResource<S> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self.state {
            TransportState::Init | TransportState::Handshake => {
                Err(io::ErrorKind::NotConnected.into())
            }
            TransportState::Active => self.session.read(buf),
            TransportState::Terminated => Err(io::ErrorKind::ConnectionAborted.into()),
        }
    }
}

impl<S: NetSession> Write for NetResource<S> {
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

impl<S: NetSession> WriteAtomic for NetResource<S> {
    fn is_ready_to_write(&self) -> bool {
        self.state == TransportState::Active
    }

    fn write_or_buffer(&mut self, buf: &[u8]) -> io::Result<()> {
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

// TODO: Replace this with from_session/into_session procedure
mod split {
    use super::*;

    #[derive(Debug, Display)]
    #[display("{error}")]
    pub struct SplitIoError<T: SplitIo> {
        pub original: T,
        pub error: io::Error,
    }

    impl<T: SplitIo + Debug> std::error::Error for SplitIoError<T> {}

    pub trait SplitIo: Sized {
        type Read: Read + Sized;
        type Write: Write + Sized;

        /// # Panics
        ///
        /// If the split operation is not possible
        fn split_io(self) -> Result<(Self::Read, Self::Write), SplitIoError<Self>>;
        fn from_split_io(read: Self::Read, write: Self::Write) -> Self;
    }

    pub struct NetReader<S: NetSession> {
        state: TransportState,
        session: <S as SplitIo>::Read,
        inbound: bool,
    }

    impl<S: NetSession> Read for NetReader<S> {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            self.session.read(buf)
        }
    }

    pub struct NetWriter<S: NetSession> {
        state: TransportState,
        session: <S as SplitIo>::Write,
        inbound: bool,
        needs_flush: bool,
    }

    impl<S: NetSession> Write for NetWriter<S> {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.needs_flush = true;
            self.session.write(buf)
        }

        fn flush(&mut self) -> io::Result<()> {
            self.needs_flush = false;
            self.session.flush()
        }
    }

    impl<S: NetSession> SplitIo for NetResource<S> {
        type Read = NetReader<S>;
        type Write = NetWriter<S>;

        fn split_io(mut self) -> Result<(Self::Read, Self::Write), SplitIoError<Self>> {
            debug_assert_eq!(self.read_buffer_len, 0);
            debug_assert_eq!(self.write_buffer.len(), 0);

            if let Err(err) = self.session.flush() {
                return Err(SplitIoError {
                    original: self,
                    error: err,
                });
            }
            let (r, w) = match self.session.split_io() {
                Err(err) => {
                    self.session = err.original;
                    return Err(SplitIoError {
                        original: self,
                        error: err.error,
                    });
                }
                Ok(s) => s,
            };
            let reader = NetReader {
                state: self.state,
                session: r,
                inbound: self.inbound,
            };
            let writer = NetWriter {
                state: self.state,
                session: w,
                inbound: self.inbound,
                needs_flush: false,
            };
            Ok((reader, writer))
        }

        fn from_split_io(read: Self::Read, write: Self::Write) -> Self {
            debug_assert_eq!(read.state, write.state);
            debug_assert_eq!(read.inbound, write.inbound);
            Self {
                state: read.state,
                inbound: read.inbound,
                session: S::from_split_io(read.session, write.session),
                write_intent: write.needs_flush,
                read_buffer: vec![0u8; READ_BUFFER_SIZE],
                read_buffer_len: 0,
                write_buffer: VecDeque::new(),
            }
        }
    }
}
use crate::connection::Proxy;
pub use split::*;
