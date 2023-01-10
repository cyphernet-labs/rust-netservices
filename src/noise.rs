use amplify::Wrapper;
use std::fmt::{Debug, Display, Formatter};
use std::hash::Hash;
use std::io::{self, Read, Write};
use std::net::{self, TcpStream, ToSocketAddrs};
use std::os::unix::io::{AsRawFd, RawFd};
use std::time::Duration;

use cyphernet::addr::{Addr, Host, PeerAddr, ToSocketAddr};
use cyphernet::crypto::{ed25519, EcPk, Ecdh};
use cyphernet::noise::framing::NoiseTranscoder;
use cyphernet::noise::xk::NoiseXkState;
use ed25519_compact::x25519;

use crate::connection::Proxy;
use crate::resources::{SplitIo, SplitIoError};
use crate::{Address, NetConnection, NetSession};

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
pub enum XkAddr<Id: PeerId, A: Address> {
    #[from]
    Partial(A),

    #[from]
    Full(PeerAddr<Id, A>),
}

impl<Id: PeerId, A: Address> Display for XkAddr<Id, A>
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

impl<Id: PeerId, A: Address> Host for XkAddr<Id, A> {}
impl<Id: PeerId, A: Address> Addr for XkAddr<Id, A> {
    fn port(&self) -> u16 {
        match self {
            XkAddr::Partial(a) => a.port(),
            XkAddr::Full(a) => a.port(),
        }
    }
}

impl<Id: PeerId, A: Address + ToSocketAddr> ToSocketAddr for XkAddr<Id, A> {
    fn to_socket_addr(&self) -> net::SocketAddr {
        match self {
            XkAddr::Partial(a) => a.to_socket_addr(),
            XkAddr::Full(a) => a.to_socket_addr(),
        }
    }
}

impl<Id: PeerId, A: Address + ToSocketAddrs> ToSocketAddrs for XkAddr<Id, A> {
    type Iter = A::Iter;

    fn to_socket_addrs(&self) -> io::Result<Self::Iter> {
        match self {
            XkAddr::Partial(a) => a.to_socket_addrs(),
            XkAddr::Full(a) => a.addr().to_socket_addrs(),
        }
    }
}

impl<Id: PeerId, A: Address> From<XkAddr<Id, A>> for net::SocketAddr
where
    A: ToSocketAddr,
{
    fn from(addr: XkAddr<Id, A>) -> Self {
        addr.to_socket_addr()
    }
}

impl<Id: PeerId, A: Address> XkAddr<Id, A> {
    pub fn upgrade(&mut self, id: Id) -> bool {
        match self {
            XkAddr::Partial(addr) => {
                *self = XkAddr::Full(PeerAddr::new(id, addr.clone()));
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
    transcoder: NoiseTranscoder<NoiseXkState>,
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

impl<E: Ecdh, S: NetConnection> Write for NoiseXk<E, S> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        // TODO: Do handshake
        if !self.established {
            self.established = true;
        }
        self.connection.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        // TODO: Do handshake
        if !self.established {
            self.established = true;
        }
        self.connection.flush()
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

impl<S: NetConnection> NetSession for NoiseXk<ed25519::PrivateKey, S> {
    type Context = ed25519::PrivateKey;
    type Connection = S;
    type Id = ed25519::PublicKey;
    type PeerAddr = PeerAddr<Self::Id, S::Addr>;
    type TransientAddr = XkAddr<Self::Id, S::Addr>;

    fn accept(connection: S, context: &Self::Context) -> io::Result<Self> {
        let ecdh =
            x25519::SecretKey::from_ed25519(context.as_inner()).expect("invalid local node key");
        Ok(Self {
            remote_addr: XkAddr::Partial(connection.remote_addr()),
            local_node: context.clone(),
            connection,
            transcoder: NoiseTranscoder::with_xk_responder(ecdh),
            established: false,
        })
    }

    fn connect_blocking<P: Proxy>(
        peer_addr: Self::PeerAddr,
        context: &Self::Context,
        proxy: &P,
    ) -> Result<Self, P::Error> {
        let ecdh =
            x25519::SecretKey::from_ed25519(context.as_inner()).expect("invalid local node key");
        let remote_key = x25519::PublicKey::from_ed25519(peer_addr.id().as_inner())
            .map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
        let socket = S::connect_blocking(peer_addr.addr().clone(), proxy)?;
        Ok(Self {
            remote_addr: XkAddr::Full(peer_addr),
            local_node: context.clone(),
            connection: socket,
            transcoder: NoiseTranscoder::with_xk_initiator(ecdh, remote_key),
            established: false,
        })
    }

    #[cfg(feature = "socket2")]
    fn connect_nonblocking<P: Proxy>(
        peer_addr: Self::PeerAddr,
        context: &Self::Context,
        proxy: &P,
    ) -> Result<Self, P::Error> {
        let ecdh =
            x25519::SecretKey::from_ed25519(context.as_inner()).expect("invalid local node key");
        let remote_key = x25519::PublicKey::from_ed25519(peer_addr.id().as_inner())
            .map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))?;
        let socket = S::connect_nonblocking(peer_addr.addr().clone(), proxy)?;
        Ok(Self {
            remote_addr: XkAddr::Full(peer_addr),
            local_node: context.clone(),
            connection: socket,
            transcoder: NoiseTranscoder::with_xk_initiator(ecdh, remote_key),
            established: false,
        })
    }

    fn session_id(&self) -> Option<Self::Id> {
        match &self.remote_addr {
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
