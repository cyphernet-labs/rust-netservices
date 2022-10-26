use std::os::unix::io::{AsRawFd, RawFd};
use std::sync::Arc;
use std::time::Duration;
use std::{io, net, time};

use crate::{OnDemand, Resource, ResourceAddr};

/// Maximum time to wait when reading from a socket.
const READ_TIMEOUT: time::Duration = Duration::from_secs(6);
/// Maximum time to wait when writing to a socket.
const WRITE_TIMEOUT: time::Duration = Duration::from_secs(3);

/// Disconnect reason originating either from the network interface or provided
/// by the network protocol state machine in form of
/// [`ReactorDispatch::DisconnectPeer`] instruction.
#[derive(Debug, Clone)]
pub enum DisconnectReason {
    /// Error while dialing the remote. This error occurs before a connection is
    /// even established. Errors of this kind are usually not transient.
    DialError(Arc<io::Error>),

    /// Error with an underlying established connection. Sometimes, reconnecting
    /// after such an error is possible.
    ConnectionError(Arc<io::Error>),

    /// Peer was disconnected due to a request from the network protocol
    /// business logic.
    OnDemand,
}

impl OnDemand for DisconnectReason {
    fn on_demand() -> Self {
        Self::OnDemand
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum TcpConnector {
    Listen(net::SocketAddr),
    Connect(net::SocketAddr),
}

impl ResourceAddr for TcpConnector {}

#[derive(Debug)]
pub enum TcpSocket {
    Listener(net::TcpListener),
    Stream(net::TcpStream),
}

impl TcpSocket {
    pub fn listen(addr: impl Into<net::SocketAddr>) -> io::Result<Self> {
        TcpSocket::connect(&TcpConnector::Listen(addr.into()))
    }

    pub fn dial(addr: impl Into<net::SocketAddr>) -> io::Result<Self> {
        TcpSocket::connect(&TcpConnector::Connect(addr.into()))
    }
}

impl Resource for TcpSocket {
    type Addr = TcpConnector;
    type DisconnectReason = DisconnectReason;
    type Error = io::Error;

    fn addr(&self) -> Self::Addr {
        match self {
            TcpSocket::Listener(listener) => TcpConnector::Listen(
                listener
                    .local_addr()
                    .expect("TCP must always know local address"),
            ),
            TcpSocket::Stream(stream) => TcpConnector::Connect(
                stream
                    .peer_addr()
                    .expect("TCP stream always has remote address"),
            ),
        }
    }

    fn connect(addr: &Self::Addr) -> Result<Self, Self::Error> {
        match addr {
            TcpConnector::Listen(addr) => {
                let listener = net::TcpListener::bind(addr)?;
                listener.set_nonblocking(true)?;
                Ok(TcpSocket::Listener(listener))
            }
            TcpConnector::Connect(addr) => {
                use socket2::{Domain, Socket, Type};

                let domain = if addr.is_ipv4() {
                    Domain::IPV4
                } else {
                    Domain::IPV6
                };
                let sock = Socket::new(domain, Type::STREAM, None)?;

                sock.set_read_timeout(Some(READ_TIMEOUT))?;
                sock.set_write_timeout(Some(WRITE_TIMEOUT))?;
                sock.set_nonblocking(true)?;

                match sock.connect(&(*addr).into()) {
                    Ok(()) => {}
                    Err(e) if e.raw_os_error() == Some(libc::EINPROGRESS) => {}
                    Err(e) if e.raw_os_error() == Some(libc::EALREADY) => {
                        return Err(io::Error::from(io::ErrorKind::AlreadyExists))
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => {}
                    Err(e) => return Err(e),
                }
                Ok(TcpSocket::Stream(sock.into()))
            }
        }
    }

    fn disconnect(&mut self) -> Result<(), Self::Error> {
        todo!()
    }
}

impl AsRawFd for TcpSocket {
    fn as_raw_fd(&self) -> RawFd {
        match self {
            TcpSocket::Listener(listener) => listener.as_raw_fd(),
            TcpSocket::Stream(stream) => stream.as_raw_fd(),
        }
    }
}

impl io::Read for TcpSocket {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self {
            TcpSocket::Listener(_) => Err(io::ErrorKind::NotConnected.into()),
            TcpSocket::Stream(stream) => stream.read(buf),
        }
    }
}

impl io::Write for TcpSocket {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            TcpSocket::Listener(_) => Err(io::ErrorKind::NotConnected.into()),
            TcpSocket::Stream(stream) => stream.write(buf),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            TcpSocket::Listener(_) => Err(io::ErrorKind::NotConnected.into()),
            TcpSocket::Stream(stream) => stream.flush(),
        }
    }
}
