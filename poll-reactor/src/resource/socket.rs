use std::collections::VecDeque;
use std::fmt::Debug;
use std::io::Error;
use std::marker::PhantomData;
use std::os::unix::io::{AsRawFd, RawFd};
use std::{io, net};

use super::Resource;
use crate::poller::IoEv;
use crate::resource::{READ_TIMEOUT, WRITE_TIMEOUT};
use crate::stream::{Frame, NetListener, NetStream};

pub trait NetSession<S: NetStream>: NetStream {
    type Context;

    fn with(stream: S, context: &Self::Context) -> Self;

    fn remote_addr(&self) -> S::Addr;
}

#[derive(Debug)]
pub struct NetAccept<L: NetListener<Stream = N>, S: NetSession<N>, N: NetStream> {
    session_context: S::Context,
    listener: L,
    spawned: VecDeque<S>,
}

impl<L: NetListener<Stream = N>, S: NetSession<N>, N: NetStream> AsRawFd for NetAccept<L, S, N> {
    fn as_raw_fd(&self) -> RawFd {
        self.listener.as_raw_fd()
    }
}

impl<L: NetListener<Stream = N>, S: NetSession<N>, N: NetStream> io::Write for NetAccept<L, S, N> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        panic!("must not write to network listener")
    }

    fn flush(&mut self) -> io::Result<()> {
        panic!("must not write to network listener")
    }
}

impl<L: NetListener<Stream = N>, S: NetSession<N>, N: NetStream> Resource for NetAccept<L, S, N> {
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

    fn send(&mut self, msg: Self::Message) -> Result<(), Error> {
        panic!("must not send messages to the network listener")
    }

    fn disconnect(self) {
        // We disconnect by dropping the self
    }
}

impl<L: NetListener<Stream = N>, S: NetSession<N>, N: NetStream> Iterator for NetAccept<L, S, N> {
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

pub struct NetTransport<S: NetSession<N>, N: NetStream, F: Frame> {
    session: S,
    framer: F,
    events: VecDeque<SessionEvent<F::Message>>,
    _phantom: PhantomData<N>,
}

impl<S: NetSession<N>, N: NetStream, F: Frame> AsRawFd for NetTransport<S, N, F> {
    fn as_raw_fd(&self) -> RawFd {
        self.session.as_raw_fd()
    }
}

impl<S: NetSession<N>, N: NetStream, F: Frame> NetTransport<S, N, F> {
    pub fn with(session: S, framer: F) -> Self {
        Self {
            session,
            framer,
            events: empty!(),
            _phantom: default!(),
        }
    }
}

impl<S: NetSession<N>, N: NetStream, F: Frame> Resource for NetTransport<S, N, F> {
    type Id = net::SocketAddr;
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

impl<S: NetSession<N>, N: NetStream, F: Frame> Iterator for NetTransport<S, N, F> {
    type Item = SessionEvent<F::Message>;

    fn next(&mut self) -> Option<Self::Item> {
        self.events.pop_front()
    }
}
