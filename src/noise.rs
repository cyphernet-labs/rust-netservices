use std::fmt::{Debug, Display, Formatter};
use std::hash::Hash;
use std::io::{self, Read, Write};
use std::net::{self, TcpStream};
use std::os::unix::io::{AsRawFd, RawFd};
use std::time::Duration;

use cyphernet::addr::{Addr, PeerAddr, ToSocketAddr};
use cyphernet::crypto::{EcPk, Ecdh};
use reactor::{IoStatus, ReadNonblocking, WriteNonblocking};

use crate::wire::{SplitIo, SplitIoError};
use crate::{NetConnection, NetSession, ResAddr};

pub trait PeerId: EcPk {}
impl<T> PeerId for T where T: EcPk {}

#[derive(Getters, Clone, Eq, PartialEq)]
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
    pub fn upgrade(&mut self, id: Id) -> bool {
        match self {
            XkAddr::Partial(addr) => {
                *self = XkAddr::Full(PeerAddr::new(id, *addr));
                true
            }
            XkAddr::Full(_) => false,
        }
    }

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
    established: bool,
}

impl<E: Ecdh, S: NetConnection> AsRawFd for NoiseXk<E, S> {
    fn as_raw_fd(&self) -> RawFd {
        self.connection.as_raw_fd()
    }
}

impl<E: Ecdh, S: NetConnection> Read for NoiseXk<E, S> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        // TODO: Do handshake
        if !self.established {
            self.established = true;
            self.remote_addr.upgrade(E::Pk::generator());
            return Ok(0);
        }
        self.connection.read(buf)
    }
}

impl<E: Ecdh, S: NetConnection> ReadNonblocking for NoiseXk<E, S> {
    fn set_read_nonblocking(&mut self, timeout: Option<Duration>) -> io::Result<()> {
        self.connection.set_read_nonblocking(timeout)
    }

    fn read_nonblocking(&mut self, buf: &mut [u8]) -> IoStatus {
        let established = self.established;
        match self.read(buf) {
            // If we get zero bytes read as a return value after the handshake
            // it means the client has made an ordered shutdown
            Ok(0) if established => IoStatus::Shutdown,
            Ok(len) => IoStatus::Success(len),
            Err(err) if err.kind() == io::ErrorKind::WouldBlock => IoStatus::WouldBlock,
            Err(err) => IoStatus::Err(err),
        }
    }
}

impl<E: Ecdh, S: NetConnection> Write for NoiseXk<E, S> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        // TODO: Do handshake
        if !self.established {
            self.established = true;
        }
        self.connection.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.connection.flush()
    }
}

impl<E: Ecdh, S: NetConnection> WriteNonblocking for NoiseXk<E, S> {
    fn set_write_nonblocking(&mut self, timeout: Option<Duration>) -> io::Result<()> {
        self.connection.set_write_nonblocking(timeout)
    }

    fn can_write(&self) -> bool {
        self.established
    }
}

impl<E: Ecdh, S: NetConnection> SplitIo for NoiseXk<E, S> {
    type Read = Self;
    type Write = Self;

    fn split_io(mut self) -> Result<(Self::Read, Self::Write), SplitIoError<Self>> {
        todo!()
        /*
        if !self.established {
            return Err(SplitIoError {
                original: self,
                error: io::ErrorKind::NotConnected.into(),
            });
        }

        let (a, b) = match self.connection.split_io() {
            Ok((a, b)) => (a, b),
            Err(SplitIoError { original, error }) => {
                self.connection = original;
                return Err(SplitIoError {
                    original: self,
                    error,
                });
            }
        };
        self.connection = b;
        Ok((
            Self {
                remote_addr: self.remote_addr.clone(),
                local_node: self.local_node.clone(),
                connection: a,
                established: self.established,
            },
            self,
        ))
         */
    }

    fn from_split_io(read: Self::Read, write: Self::Write) -> Self {
        todo!();
        /*
        debug_assert!(read.established);
        debug_assert_eq!(read.established, write.established);
        Self {
            remote_addr: write.remote_addr,
            local_node: write.local_node,
            connection: S::from_split_io(read.connection, write.connection),
            established: read.established & write.established,
        }
         */
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
    type TransientAddr = XkAddr<Self::Id, S::Addr>;

    fn accept(connection: S, context: &Self::Context) -> io::Result<Self> {
        Ok(Self {
            remote_addr: XkAddr::Partial(connection.remote_addr()),
            local_node: context.clone(),
            connection,
            established: false,
        })
    }

    fn connect(
        peer_addr: Self::PeerAddr,
        context: &Self::Context,
        nonblocking: bool,
    ) -> io::Result<Self> {
        let socket = S::connect(peer_addr.addr().clone(), nonblocking)?;
        Ok(Self {
            remote_addr: XkAddr::Full(peer_addr),
            local_node: context.clone(),
            connection: socket,
            established: false,
        })
    }

    fn id(&self) -> Option<Self::Id> {
        match self.remote_addr {
            XkAddr::Partial(_) => None,
            XkAddr::Full(a) => Some(*a.id()),
        }
    }

    fn handshake_completed(&self) -> bool {
        self.established
    }

    fn transient_addr(&self) -> Self::TransientAddr {
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
