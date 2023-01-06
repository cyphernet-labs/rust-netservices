use std::fmt::{Debug, Display, Formatter};
use std::hash::Hash;
use std::io::{self, Read, Write};
use std::net::{self, TcpStream};
use std::os::unix::io::{AsRawFd, RawFd};
use std::time::Duration;

use cyphernet::addr::{Addr, PeerAddr, ToSocketAddr};
use cyphernet::crypto::{EcPk, Ecdh};

use crate::wire::SplitIo;
use crate::{NetConnection, NetSession, ResAddr};

pub trait PeerId: EcPk {}
impl<T> PeerId for T where T: EcPk {}

#[derive(Clone, Eq, PartialEq)]
pub struct NodeKeys<E: Ecdh> {
    pk: E::Pk,
    ecdh: E,
}

impl<E: Ecdh> From<E> for NodeKeys<E> {
    fn from(ecdh: E) -> Self {
        NodeKeys {
            pk: ecdh.to_pk(),
            ecdh,
        }
    }
}

#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Debug, From)]
pub enum XkAddr<Id: PeerId, A: ResAddr> {
    #[from]
    Partial(A),

    #[from]
    Full(PeerAddr<Id, A>),
}

impl<Id: PeerId, A: ResAddr> Display for XkAddr<Id, A>
where
    Id: Display,
{
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            XkAddr::Partial(addr) => Display::fmt(addr, f),
            XkAddr::Full(addr) => Display::fmt(addr, f),
        }
    }
}

impl<Id: PeerId, A: ResAddr> Addr for XkAddr<Id, A> {
    fn port(&self) -> u16 {
        match self {
            XkAddr::Partial(a) => a.port(),
            XkAddr::Full(a) => a.port(),
        }
    }
}

impl<Id: PeerId, A: ResAddr + ToSocketAddr> ToSocketAddr for XkAddr<Id, A> {
    fn to_socket_addr(&self) -> net::SocketAddr {
        match self {
            XkAddr::Partial(a) => a.to_socket_addr(),
            XkAddr::Full(a) => a.to_socket_addr(),
        }
    }
}

impl<Id: PeerId, A: ResAddr> From<XkAddr<Id, A>> for net::SocketAddr
where
    A: ToSocketAddr,
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

    pub fn expect_peer_addr(&self) -> &PeerAddr<Id, A> {
        match self {
            XkAddr::Partial(_) => panic!("handshake is not complete"),
            XkAddr::Full(addr) => addr,
        }
    }
}

pub struct NoiseXk<E: Ecdh, S: NetConnection = TcpStream> {
    remote_addr: XkAddr<E::Pk, S::Addr>,
    local_node: E,
    connection: S,
}

impl<E: Ecdh, S: NetConnection> AsRawFd for NoiseXk<E, S> {
    fn as_raw_fd(&self) -> RawFd {
        self.connection.as_raw_fd()
    }
}

impl<E: Ecdh, S: NetConnection> Read for NoiseXk<E, S> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        // TODO: Do handshake
        self.connection.read(buf)
    }
}

impl<E: Ecdh, S: NetConnection> Write for NoiseXk<E, S> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        // TODO: Do handshake
        self.connection.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.connection.flush()
    }
}

impl<E: Ecdh, S: NetConnection> SplitIo for NoiseXk<E, S> {
    type Read = <S as SplitIo>::Read;
    type Write = <S as SplitIo>::Write;

    fn split_io(self) -> (Self::Read, Self::Write) {
        todo!()
    }

    fn from_split_io(read: Self::Read, write: Self::Write) -> Self {
        todo!()
    }
}

impl<E: Ecdh, S: NetConnection> NetSession for NoiseXk<E, S>
where
    E: Send + Clone,
    E::Pk: Send + Copy,
{
    type Context = E;
    type Connection = S;
    type Id = E::Pk;
    type PeerAddr = PeerAddr<Self::Id, S::Addr>;
    type TransitionAddr = XkAddr<Self::Id, S::Addr>;

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

    fn expect_id(&self) -> Self::Id {
        match self.remote_addr {
            XkAddr::Partial(_) => {
                unreachable!("NoiseXk::id must not be called until the handshake is complete")
            }
            XkAddr::Full(a) => *a.id(),
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
