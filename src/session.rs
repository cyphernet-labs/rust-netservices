use std::io;
use std::net;
use std::os::unix::io::{AsRawFd, RawFd};
use std::time::Duration;

use cyphernet::addr::Addr;

use crate::wire::SplitIo;
use crate::{NetConnection, StreamNonblocking};

pub trait NetSession: StreamNonblocking + SplitIo + AsRawFd + Send + Sized {
    type Context: Send;
    type Connection: NetConnection;
    /// A unique identifier of the session. Usually a part of a transition address.
    type Id: Send;
    /// Address used for outgoing connections. May not be known initially for the incoming
    /// connections
    type PeerAddr: Addr;
    /// Address which combines what is known for both incoming and outgoing connections.
    type TransientAddr: Addr;

    fn accept(connection: Self::Connection, context: &Self::Context) -> io::Result<Self>;
    fn connect(
        addr: Self::PeerAddr,
        context: &Self::Context,
        nonblocking: bool,
    ) -> io::Result<Self>;

    fn session_id(&self) -> Option<Self::Id>;
    fn expect_id(&self) -> Self::Id {
        self.session_id()
            .expect("net session id is not present when expected")
    }

    fn handshake_completed(&self) -> bool;

    fn transient_addr(&self) -> Self::TransientAddr;
    fn peer_addr(&self) -> Option<Self::PeerAddr>;
    fn local_addr(&self) -> <Self::Connection as NetConnection>::Addr;

    fn read_timeout(&self) -> io::Result<Option<Duration>>;
    fn write_timeout(&self) -> io::Result<Option<Duration>>;
    fn set_read_timeout(&mut self, dur: Option<Duration>) -> io::Result<()>;
    fn set_write_timeout(&mut self, dur: Option<Duration>) -> io::Result<()>;

    fn set_nonblocking(&mut self, nonblocking: bool) -> io::Result<()>;

    fn disconnect(self) -> io::Result<()>;
}

#[cfg(feature = "socket2")]
impl NetSession for net::TcpStream {
    type Context = ();
    type Connection = Self;
    type Id = RawFd;
    type PeerAddr = net::SocketAddr;
    type TransientAddr = net::SocketAddr;

    fn accept(connection: Self::Connection, _context: &Self::Context) -> io::Result<Self> {
        Ok(connection)
    }

    fn connect(
        addr: Self::PeerAddr,
        _context: &Self::Context,
        nonblocking: bool,
    ) -> io::Result<Self> {
        NetConnection::connect(addr, nonblocking)
    }

    fn session_id(&self) -> Option<Self::Id> {
        Some(self.as_raw_fd())
    }

    fn handshake_completed(&self) -> bool {
        true
    }

    fn transient_addr(&self) -> Self::TransientAddr {
        <Self as NetConnection>::remote_addr(self)
    }

    fn peer_addr(&self) -> Option<Self::PeerAddr> {
        Some(<Self as NetConnection>::remote_addr(self))
    }

    fn local_addr(&self) -> <Self::Connection as NetConnection>::Addr {
        <Self as NetConnection>::local_addr(self)
    }

    fn read_timeout(&self) -> io::Result<Option<Duration>> {
        <Self as NetConnection>::read_timeout(self)
    }

    fn write_timeout(&self) -> io::Result<Option<Duration>> {
        <Self as NetConnection>::write_timeout(self)
    }

    fn set_read_timeout(&mut self, dur: Option<Duration>) -> io::Result<()> {
        <Self as NetConnection>::set_read_timeout(self, dur)
    }

    fn set_write_timeout(&mut self, dur: Option<Duration>) -> io::Result<()> {
        <Self as NetConnection>::set_write_timeout(self, dur)
    }

    fn set_nonblocking(&mut self, nonblocking: bool) -> io::Result<()> {
        <Self as NetConnection>::set_nonblocking(self, nonblocking)
    }

    fn disconnect(self) -> io::Result<()> {
        self.shutdown(net::Shutdown::Both)
    }
}

#[cfg(feature = "socket2")]
impl NetSession for socket2::Socket {
    type Context = ();
    type Connection = Self;
    type Id = RawFd;
    type PeerAddr = net::SocketAddr;
    type TransientAddr = net::SocketAddr;

    fn accept(connection: Self::Connection, _context: &Self::Context) -> io::Result<Self> {
        Ok(connection)
    }

    fn connect(
        addr: Self::PeerAddr,
        _context: &Self::Context,
        nonblocking: bool,
    ) -> io::Result<Self> {
        NetConnection::connect(addr, nonblocking)
    }

    fn session_id(&self) -> Option<Self::Id> {
        Some(self.as_raw_fd())
    }

    fn handshake_completed(&self) -> bool {
        true
    }

    fn transient_addr(&self) -> Self::TransientAddr {
        <Self as NetConnection>::remote_addr(self)
    }

    fn peer_addr(&self) -> Option<Self::PeerAddr> {
        Some(<Self as NetConnection>::remote_addr(self))
    }

    fn local_addr(&self) -> <Self::Connection as NetConnection>::Addr {
        <Self as NetConnection>::local_addr(self)
    }

    fn read_timeout(&self) -> io::Result<Option<Duration>> {
        <Self as NetConnection>::read_timeout(self)
    }

    fn write_timeout(&self) -> io::Result<Option<Duration>> {
        <Self as NetConnection>::write_timeout(self)
    }

    fn set_read_timeout(&mut self, dur: Option<Duration>) -> io::Result<()> {
        <Self as NetConnection>::set_read_timeout(self, dur)
    }

    fn set_write_timeout(&mut self, dur: Option<Duration>) -> io::Result<()> {
        <Self as NetConnection>::set_write_timeout(self, dur)
    }

    fn set_nonblocking(&mut self, nonblocking: bool) -> io::Result<()> {
        <Self as NetConnection>::set_nonblocking(self, nonblocking)
    }

    fn disconnect(self) -> io::Result<()> {
        self.shutdown(net::Shutdown::Both)
    }
}
