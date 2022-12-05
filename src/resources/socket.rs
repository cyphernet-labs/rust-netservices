use std::collections::VecDeque;
use std::fmt::Debug;
use std::io::Error;
use std::os::unix::io::{AsRawFd, RawFd};
use std::{io, net};

use reactor::poller::IoEv;
use reactor::resource::{Resource, ResourceId};

use crate::stream::{Frame, NetListener, NetStream, READ_TIMEOUT, WRITE_TIMEOUT};

pub trait NetSession: NetStream {
    type Context;
    type Inner: NetStream;

    fn with(stream: Self::Inner, context: &Self::Context) -> Self;

    fn remote_addr(&self) -> Self::Addr;
}

#[derive(Debug)]
pub struct NetAccept<L: NetListener<Stream = S::Inner>, S: NetSession> {
    session_context: S::Context,
    listener: L,
    spawned: VecDeque<S>,
}

impl<L: NetListener<Stream = S::Inner>, S: NetSession> AsRawFd for NetAccept<L, S> {
    fn as_raw_fd(&self) -> RawFd {
        self.listener.as_raw_fd()
    }
}

impl<L: NetListener<Stream = S::Inner>, S: NetSession> io::Write for NetAccept<L, S> {
    fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
        panic!("must not write to network listener")
    }

    fn flush(&mut self) -> io::Result<()> {
        panic!("must not write to network listener")
    }
}

impl<L: NetListener<Stream = S::Inner>, S: NetSession> Resource for NetAccept<L, S> {
    type Id = net::SocketAddr;
    type Event = S;
    type Message = (); // Indicates incoming connection

    fn id(&self) -> Self::Id {
        self.listener.local_addr()
    }

    fn handle_io(&mut self, ev: IoEv) -> usize {
        if ev.is_writable {
            let (mut stream, _) = self.listener.accept().expect("GENERATE EVENT");
            stream
                .set_read_timeout(Some(READ_TIMEOUT))
                .expect("GENERATE EVENT");
            stream
                .set_write_timeout(Some(WRITE_TIMEOUT))
                .expect("GENERATE EVENT");
            stream.set_nonblocking(true).expect("GENERATE EVENT");

            let session = S::with(stream, &self.session_context);
            self.spawned.push_back(session);
            1
        } else {
            0
        }
    }

    fn send(&mut self, _msg: Self::Message) -> Result<(), Error> {
        panic!("must not send messages to the network listener")
    }

    fn disconnect(self) {
        // We disconnect by dropping the self
    }
}

impl<L: NetListener<Stream = S::Inner>, S: NetSession> Iterator for NetAccept<L, S> {
    type Item = S;

    fn next(&mut self) -> Option<Self::Item> {
        self.spawned.pop_front()
    }
}

pub enum SessionEvent<M> {
    Connected,
    SessionEstablished,
    Message(M),
    Disconnected,
}

pub struct NetTransport<S: NetSession, F: Frame> {
    session: S,
    framer: F,
    events: VecDeque<SessionEvent<F::Message>>,
}

impl<S: NetSession, F: Frame> AsRawFd for NetTransport<S, F> {
    fn as_raw_fd(&self) -> RawFd {
        self.session.as_raw_fd()
    }
}

impl<S: NetSession, F: Frame> NetTransport<S, F> {
    pub fn with(session: S, framer: F) -> Self {
        Self {
            session,
            framer,
            events: empty!(),
        }
    }
}

impl<S: NetSession, F: Frame> Resource for NetTransport<S, F>
where
    S::Addr: ResourceId,
{
    type Id = S::Addr;
    type Event = SessionEvent<F::Message>;
    type Message = F::Message;

    fn id(&self) -> Self::Id {
        self.session.remote_addr().into()
    }

    fn handle_io(&mut self, ev: IoEv) -> usize {
        todo!()
    }

    fn send(&mut self, msg: Self::Message) -> Result<(), Error> {
        self.framer.push(msg);
        let mut buf = vec![0u8; self.framer.queue_len()];
        self.framer.read_exact(&mut buf)?;
        self.session.write_all(&buf)
    }

    fn disconnect(self) {
        // We just drop the self
    }
}

impl<S: NetSession, F: Frame> Iterator for NetTransport<S, F> {
    type Item = SessionEvent<F::Message>;

    fn next(&mut self) -> Option<Self::Item> {
        self.events.pop_front()
    }
}
