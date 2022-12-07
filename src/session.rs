use std::io;
use std::net;
use std::os::unix::io::{AsRawFd, RawFd};
use std::time::Duration;

use cyphernet::addr::Addr;
use reactor::ResourceId;

use crate::{IoStream, NetConnection};

pub trait NetSession: IoStream + AsRawFd + Sized {
    type Context;
    type Connection: NetConnection;
    /// A unique identifier of the session. Usually a part of a transition address.
    type Id: ResourceId;
    /// Address used for outgoing connections. May not be known initially for the incoming
    /// connections
    type PeerAddr: Addr;
    /// Address which combines what is known for both incoming and outgoing connections.
    type TransitionAddr: Addr;

    fn accept(connection: Self::Connection, context: &Self::Context) -> Self;
    fn connect(addr: Self::PeerAddr, context: &Self::Context) -> io::Result<Self>;

    fn id(&self) -> Self::Id;

    fn handshake_completed(&self) -> bool;

    fn transition_addr(&self) -> Self::TransitionAddr;
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
    type TransitionAddr = net::SocketAddr;

    fn accept(connection: Self::Connection, _context: &Self::Context) -> Self {
        connection
    }

    fn connect(addr: Self::PeerAddr, _context: &Self::Context) -> io::Result<Self> {
        Self::connect_nonblocking(addr)
    }

    fn id(&self) -> Self::Id {
        self.as_raw_fd()
    }

    fn handshake_completed(&self) -> bool {
        true
    }

    fn transition_addr(&self) -> Self::TransitionAddr {
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
    type TransitionAddr = net::SocketAddr;

    fn accept(connection: Self::Connection, _context: &Self::Context) -> Self {
        connection
    }

    fn connect(addr: Self::PeerAddr, _context: &Self::Context) -> io::Result<Self> {
        Self::connect_nonblocking(addr)
    }

    fn id(&self) -> Self::Id {
        self.as_raw_fd()
    }

    fn handshake_completed(&self) -> bool {
        true
    }

    fn transition_addr(&self) -> Self::TransitionAddr {
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
