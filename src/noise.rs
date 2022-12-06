use std::io::{self, Read, Write};
use std::net;
use std::net::TcpStream;
use std::os::unix::io::{AsRawFd, RawFd};
use std::time::Duration;

use cyphernet::addr::{Addr, LocalNode, PeerAddr};
use cyphernet::crypto::Ec;

use crate::{NetConnection, NetSession};

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

pub struct NoiseXk<C: Ec, S: NetConnection = TcpStream> {
    remote_addr: XkAddr<C::PubKey, S::Addr>,
    local_node: LocalNode<C>,
    connection: S,
}

impl<C: Ec, S: NetConnection> AsRawFd for NoiseXk<C, S> {
    fn as_raw_fd(&self) -> RawFd {
        self.connection.as_raw_fd()
    }
}

impl<C: Ec, S: NetConnection> Read for NoiseXk<C, S> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        // TODO: Do handshake
        self.connection.read(buf)
    }
}

impl<C: Ec, S: NetConnection> Write for NoiseXk<C, S> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        // TODO: Do handshake
        self.connection.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.connection.flush()
    }
}

impl<C: Ec, S: NetConnection> NetSession for NoiseXk<C, S> {
    type Context = LocalNode<C>;
    type Connection = S;
    type PeerAddr = PeerAddr<C::PubKey, S::Addr>;
    type TransitionAddr = XkAddr<C::PubKey, S::Addr>;

    fn accept(connection: S, context: &Self::Context) -> Self {
        Self {
            remote_addr: XkAddr::Partial(connection.peer_addr()),
            local_node: context.clone(),
            connection,
        }
    }

    fn connect(peer_addr: Self::PeerAddr, context: &Self::Context) -> io::Result<Self> {
        let socket = S::connect_nonblocking(peer_addr.addr().clone())?;
        Ok(Self {
            remote_addr: XkAddr::Full(peer_addr),
            local_node: context.clone(),
            connection: socket,
        })
    }

    fn handshake_completed(&self) -> bool {
        match self.remote_addr {
            XkAddr::Partial(_) => false,
            XkAddr::Full(_) => true,
        }
    }

    fn transition_addr(&self) -> Self::TransitionAddr {
        self.remote_addr.clone()
    }

    fn peer_addr(&self) -> Option<Self::PeerAddr> {
        match self.remote_addr {
            XkAddr::Partial(_) => None,
            XkAddr::Full(ref addr) => Some(addr.clone()),
        }
    }

    fn local_addr(&self) -> <Self::Connection as NetConnection>::Addr {
        self.connection.local_addr()
    }

    fn read_timeout(&self) -> io::Result<Option<Duration>> {
        self.connection.read_timeout()
    }

    fn write_timeout(&self) -> io::Result<Option<Duration>> {
        self.connection.write_timeout()
    }

    fn set_read_timeout(&mut self, dur: Option<Duration>) -> io::Result<()> {
        self.connection.set_read_timeout(dur)
    }

    fn set_write_timeout(&mut self, dur: Option<Duration>) -> io::Result<()> {
        self.connection.set_write_timeout(dur)
    }

    fn set_nonblocking(&mut self, nonblocking: bool) -> io::Result<()> {
        self.connection.set_nonblocking(nonblocking)
    }

    fn disconnect(mut self) -> io::Result<()> {
        self.connection.shutdown(net::Shutdown::Both)
    }
}
