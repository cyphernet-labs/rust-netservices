use std::fmt::{Debug, Display};
use std::hash::Hash;
use std::io::{self, Read, Write};
use std::net::{self, TcpStream};
use std::os::unix::io::{AsRawFd, RawFd};
use std::time::Duration;

use cyphernet::addr::{Addr, LocalNode, PeerAddr};
use cyphernet::crypto::Ec;
use reactor::ResourceId;

use crate::{NetConnection, NetSession, ResAddr};

pub trait PeerId: Copy + Ord + Eq + Hash + Debug + Display {}
impl<T> PeerId for T where T: Copy + Ord + Eq + Hash + Debug + Display {}

#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Debug, Display)]
#[display(inner)]
pub enum XkId<Id: PeerId> {
    Final(Id),
    Temporary(net::SocketAddr),
}

impl<Id: PeerId> ResourceId for XkId<Id> {}

#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Debug, Display, From)]
#[display(inner)]
pub enum XkAddr<Id: PeerId, A: ResAddr> {
    #[from]
    Partial(A),

    #[from]
    Full(PeerAddr<Id, A>),
}

impl<Id: PeerId, A: ResAddr> Addr for XkAddr<Id, A> {
    fn port(&self) -> u16 {
        match self {
            XkAddr::Partial(a) => a.port(),
            XkAddr::Full(a) => a.port(),
        }
    }

    fn to_socket_addr(&self) -> net::SocketAddr {
        match self {
            XkAddr::Partial(a) => a.to_socket_addr(),
            XkAddr::Full(a) => a.to_socket_addr(),
        }
    }
}

impl<Id: PeerId, A: ResAddr> From<XkAddr<Id, A>> for net::SocketAddr
where
    A: Into<net::SocketAddr>,
{
    fn from(addr: XkAddr<Id, A>) -> Self {
        addr.to_socket_addr()
    }
}

impl<Id: PeerId, A: ResAddr> XkAddr<Id, A> {
    pub fn as_addr(&self) -> &A {
        match self {
            XkAddr::Partial(a) => a,
            XkAddr::Full(a) => a.addr(),
        }
    }

    pub fn expect_peer_addr(&self) -> PeerAddr<Id, A> {
        match self {
            XkAddr::Partial(_) => panic!("handshake is not complete"),
            XkAddr::Full(addr) => addr.clone(),
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
    type Id = XkId<C::PubKey>;
    type PeerAddr = PeerAddr<C::PubKey, S::Addr>;
    type TransitionAddr = XkAddr<C::PubKey, S::Addr>;

    fn accept(connection: S, context: &Self::Context) -> Self {
        Self {
            remote_addr: XkAddr::Partial(connection.remote_addr()),
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

    fn id(&self) -> Self::Id {
        match self.remote_addr {
            XkAddr::Partial(a) => XkId::Temporary(a.to_socket_addr()),
            XkAddr::Full(a) => XkId::Final(*a.id()),
        }
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
