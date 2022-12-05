use std::io::{self, Read, Write};
use std::net;
use std::os::unix::io::{AsRawFd, RawFd};
use std::time::Duration;

use cyphernet::addr::{Addr, LocalNode, NodeId, PeerAddr};
use cyphernet::crypto::Ec;

use crate::{NetSession, NetStream};

#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Debug, From)]
pub enum XkAddr<Id, A: Addr + Clone> {
    #[from]
    Partial(A),

    #[from]
    Full(PeerAddr<Id, A>),
}

impl<Id, A: Addr + Clone> Addr for XkAddr<Id, A> {
    fn port(&self) -> u16 {
        match self {
            XkAddr::Partial(a) => a.port(),
            XkAddr::Full(a) => a.port(),
        }
    }
}

impl<Id, A: Addr + Clone> From<XkAddr<Id, A>> for net::SocketAddr
where
    for<'a> &'a A: Into<net::SocketAddr>,
{
    fn from(addr: XkAddr<Id, A>) -> Self {
        addr.to_socket_addr()
    }
}

impl<Id, A: Addr + Clone> XkAddr<Id, A> {
    pub fn addr(&self) -> A {
        match self {
            XkAddr::Partial(a) => a.clone(),
            XkAddr::Full(a) => a.addr().clone(),
        }
    }

    pub fn to_socket_addr(&self) -> net::SocketAddr
    where
        for<'a> &'a A: Into<net::SocketAddr>,
    {
        match self {
            XkAddr::Partial(a) => a.into(),
            XkAddr::Full(a) => a.to_socket_addr(),
        }
    }
}

pub struct NoiseXk<C: Ec, S: NetSession> {
    remote_addr: XkAddr<NodeId<C>, S::RemoteAddr>,
    local_node: LocalNode<C>,
    socket: S,
}

impl<C: Ec, S: NetSession> AsRawFd for NoiseXk<C, S> {
    fn as_raw_fd(&self) -> RawFd {
        self.socket.as_raw_fd()
    }
}

impl<C: Ec, S: NetSession> Read for NoiseXk<C, S> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.socket.read(buf)
    }
}

impl<C: Ec, S: NetSession> Write for NoiseXk<C, S> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.socket.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.socket.flush()
    }
}

impl<C: Ec, S: NetSession<Context = ()>> NetSession for NoiseXk<C, S>
where
    S::RemoteAddr: From<S::Addr>,
{
    type Context = LocalNode<C>;
    type Inner = S;
    type RemoteAddr = PeerAddr<NodeId<C>, S::RemoteAddr>;

    fn accept(socket: S, context: &Self::Context) -> Self {
        Self {
            remote_addr: XkAddr::Partial(socket.peer_addr().into()),
            local_node: context.clone(),
            socket,
        }
    }

    fn connect(peer_addr: Self::RemoteAddr, context: &Self::Context) -> io::Result<Self> {
        let socket = S::connect(peer_addr.addr().clone(), &())?;
        Ok(Self {
            remote_addr: XkAddr::Full(peer_addr),
            local_node: context.clone(),
            socket,
        })
    }
}

impl<C: Ec, S: NetSession> NetStream for NoiseXk<C, S>
where
    S::RemoteAddr: From<S::Addr>,
{
    type Addr = S::RemoteAddr;

    fn shutdown(&mut self, how: net::Shutdown) -> io::Result<()> {
        self.socket.shutdown(how)
    }

    fn peer_addr(&self) -> Self::Addr {
        self.remote_addr.addr().clone()
    }

    fn local_addr(&self) -> Self::Addr {
        self.socket.local_addr().into()
    }

    fn set_read_timeout(&mut self, dur: Option<Duration>) -> io::Result<()> {
        self.socket.set_read_timeout(dur)
    }

    fn set_write_timeout(&mut self, dur: Option<Duration>) -> io::Result<()> {
        self.socket.set_write_timeout(dur)
    }

    fn read_timeout(&self) -> io::Result<Option<Duration>> {
        self.socket.read_timeout()
    }

    fn write_timeout(&self) -> io::Result<Option<Duration>> {
        self.socket.write_timeout()
    }

    fn peek(&self, buf: &mut [u8]) -> io::Result<usize> {
        self.socket.peek(buf)
    }

    fn set_nodelay(&mut self, nodelay: bool) -> io::Result<()> {
        self.socket.set_nodelay(nodelay)
    }

    fn nodelay(&self) -> io::Result<bool> {
        self.socket.nodelay()
    }

    fn set_ttl(&mut self, ttl: u32) -> io::Result<()> {
        self.socket.set_ttl(ttl)
    }

    fn ttl(&self) -> io::Result<u32> {
        self.socket.ttl()
    }

    fn set_nonblocking(&mut self, nonblocking: bool) -> io::Result<()> {
        self.socket.set_nonblocking(nonblocking)
    }

    fn try_clone(&self) -> io::Result<Self> {
        Ok(Self {
            remote_addr: self.remote_addr.clone(),
            local_node: self.local_node.clone(),
            socket: self.socket.try_clone()?,
        })
    }

    fn take_error(&self) -> io::Result<Option<io::Error>> {
        self.socket.take_error()
    }
}
