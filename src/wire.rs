use std::collections::VecDeque;
use std::fmt::Debug;
use std::net::TcpListener;
use std::os::unix::io::{AsRawFd, RawFd};
use std::{io, net};

use reactor::poller::IoEv;
use reactor::Resource;

use crate::{Frame, NetConnection, NetListener, NetSession};

/// Socket read buffer size.
const READ_BUFFER_SIZE: usize = 1024 * 192;
/// Maximum time to wait when reading from a socket.
const READ_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(6);
/// Maximum time to wait when writing to a socket.
const WRITE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);

#[derive(Debug)]
pub enum ListenerEvent<S: NetSession> {
    Accepted(S),
    Error(io::Error),
}

#[derive(Debug)]
pub struct NetAccept<S: NetSession, L: NetListener<Stream = S::Connection> = TcpListener> {
    session_context: S::Context,
    listener: L,
    events: VecDeque<ListenerEvent<S>>,
}

impl<L: NetListener<Stream = S::Connection>, S: NetSession> AsRawFd for NetAccept<S, L> {
    fn as_raw_fd(&self) -> RawFd {
        self.listener.as_raw_fd()
    }
}

impl<L: NetListener<Stream = S::Connection>, S: NetSession> io::Write for NetAccept<S, L> {
    fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
        panic!("must not write to network listener")
    }

    fn flush(&mut self) -> io::Result<()> {
        panic!("must not write to network listener")
    }
}

impl<L: NetListener<Stream = S::Connection>, S: NetSession> NetAccept<S, L> {
    pub fn bind(addr: impl Into<net::SocketAddr>, session_context: S::Context) -> io::Result<Self> {
        let listener = L::bind(addr)?;
        listener.set_nonblocking(true)?;
        Ok(Self {
            session_context,
            listener,
            events: empty!(),
        })
    }

    fn handle_accept(&mut self) -> io::Result<()> {
        let mut stream = self.listener.accept()?;
        stream.set_read_timeout(Some(READ_TIMEOUT))?;
        stream.set_write_timeout(Some(WRITE_TIMEOUT))?;
        stream.set_nonblocking(true)?;
        let session = S::accept(stream, &self.session_context);
        self.events.push_back(ListenerEvent::Accepted(session));
        Ok(())
    }
}

impl<L: NetListener<Stream = S::Connection>, S: NetSession> Resource for NetAccept<S, L> {
    type Id = net::SocketAddr;
    type Event = ListenerEvent<S>;
    type Message = (); // Indicates incoming connection

    fn id(&self) -> Self::Id {
        self.listener.local_addr()
    }

    fn handle_io(&mut self, ev: IoEv) -> usize {
        if ev.is_writable {
            if let Err(err) = self.handle_accept() {
                self.events.push_back(ListenerEvent::Error(err));
                0
            } else {
                1
            }
        } else {
            0
        }
    }

    fn send(&mut self, _msg: Self::Message) -> io::Result<()> {
        panic!("must not send messages to the network listener")
    }

    fn disconnect(self) {
        // We disconnect by dropping the self
    }
}

impl<L: NetListener<Stream = S::Connection>, S: NetSession> Iterator for NetAccept<S, L> {
    type Item = ListenerEvent<S>;

    fn next(&mut self) -> Option<Self::Item> {
        self.events.pop_front()
    }
}

pub enum SessionEvent<S: NetSession, F: Frame> {
    SessionEstablished(S::PeerAddr),
    Message(F::Message),
    FrameFailure(F::Error),
    ConnectionFailure(io::Error),
    Disconnected,
}

pub struct NetTransport<S: NetSession, F: Frame> {
    session: S,
    framer: F,
    events: VecDeque<SessionEvent<S, F>>,
}

impl<S: NetSession, F: Frame> AsRawFd for NetTransport<S, F> {
    fn as_raw_fd(&self) -> RawFd {
        self.session.as_raw_fd()
    }
}

impl<S: NetSession, F: Frame> NetTransport<S, F> {
    pub fn upgrade(mut session: S) -> io::Result<Self> {
        session.set_read_timeout(Some(READ_TIMEOUT))?;
        session.set_write_timeout(Some(WRITE_TIMEOUT))?;
        session.set_nonblocking(true)?;
        Ok(Self {
            session,
            framer: default!(),
            events: empty!(),
        })
    }

    pub fn connect(addr: S::PeerAddr, context: &S::Context) -> io::Result<Self> {
        let session = S::connect(addr, context)?;
        Self::upgrade(session)
    }

    fn handle_writable(&mut self) {
        if let Err(err) = self.session.flush() {
            self.events.push_back(SessionEvent::ConnectionFailure(err));
        }
    }

    fn handle_readable(&mut self) {
        // We need to save the state before doing the read below
        let was_established = self.session.handshake_completed();
        let mut buffer = [0; READ_BUFFER_SIZE];
        let res = self.session.read(&mut buffer);
        if !was_established {
            if let Some(peer_addr) = self.session.peer_addr() {
                self.events
                    .push_back(SessionEvent::SessionEstablished(peer_addr));
            }
        }
        match res {
            Ok(0) if !was_established => {
                // Do nothing since we haven't established session yet
            }
            // Nb. Since `poll`, which this reactor is based on, is *level-triggered*,
            // we will be notified again if there is still data to be read on the socket.
            // Hence, there is no use in putting this socket read in a loop, as the second
            // invocation would likely block.
            Ok(0) => {
                // If we get zero bytes read as a return value, it means the peer has
                // performed an orderly shutdown.
                self.events.push_back(SessionEvent::Disconnected)
            }
            Ok(len) => {
                self.framer
                    .write_all(&buffer[..len])
                    .expect("in-memory writer");
                loop {
                    match self.framer.pop() {
                        Ok(Some(msg)) => self.events.push_back(SessionEvent::Message(msg)),
                        Ok(None) => break,
                        Err(err) => {
                            self.events.push_back(SessionEvent::FrameFailure(err));
                            break;
                        }
                    }
                }
            }
            Err(err) if err.kind() == io::ErrorKind::WouldBlock => {
                // This shouldn't normally happen, since this function is only called
                // when there's data on the socket. We leave it here in case external
                // conditions change.
                unreachable!()
            }
            Err(err) => self.events.push_back(SessionEvent::ConnectionFailure(err)),
        }
    }
}

impl<S: NetSession, F: Frame> Resource for NetTransport<S, F>
where
    S::TransitionAddr: Into<net::SocketAddr>,
{
    type Id = net::SocketAddr;
    type Event = SessionEvent<S, F>;
    type Message = F::Message;

    fn id(&self) -> Self::Id {
        self.session.transition_addr().into()
    }

    fn handle_io(&mut self, ev: IoEv) -> usize {
        let len = self.events.len();
        if ev.is_writable {
            self.handle_writable();
        } else if ev.is_readable {
            self.handle_readable();
        }
        // TODO: Handle exceptions like hangouts etc
        self.events.len() - len
    }

    fn send(&mut self, msg: Self::Message) -> io::Result<()> {
        self.framer.push(msg);
        let mut buf = vec![0u8; self.framer.queue_len()];
        self.framer.read_exact(&mut buf).expect(
            "queue length reported by framer doesn't exceeds the length of the returned data",
        );
        self.session.write_all(&buf)
    }

    fn disconnect(self) {
        // We just drop the self
    }
}

impl<S: NetSession, F: Frame> Iterator for NetTransport<S, F> {
    type Item = SessionEvent<S, F>;

    fn next(&mut self) -> Option<Self::Item> {
        self.events.pop_front()
    }
}
