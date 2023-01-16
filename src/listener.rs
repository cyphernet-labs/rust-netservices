use std::io;
use std::net::{SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::os::unix::io::AsRawFd;

use crate::connection::NetConnection;

pub trait NetListener: AsRawFd + Send {
    type Stream: NetConnection;

    fn bind(addr: &impl ToSocketAddrs) -> io::Result<Self>
    where
        Self: Sized;

    fn accept(&self) -> io::Result<Self::Stream>;

    fn local_addr(&self) -> SocketAddr;

    fn ttl(&self) -> io::Result<u32>;
    fn set_ttl(&self, ttl: u32) -> io::Result<()>;

    fn set_nonblocking(&self, nonblocking: bool) -> io::Result<()>;

    fn try_clone(&self) -> io::Result<Self>
    where
        Self: Sized;
    fn take_error(&self) -> io::Result<Option<io::Error>>;
}

impl NetListener for TcpListener {
    type Stream = TcpStream;

    fn bind(addr: &impl ToSocketAddrs) -> io::Result<Self>
    where
        Self: Sized,
    {
        TcpListener::bind(addr)
    }

    fn accept(&self) -> io::Result<Self::Stream> {
        Ok(TcpListener::accept(self)?.0)
    }

    fn local_addr(&self) -> SocketAddr {
        TcpListener::local_addr(self).expect("TCP listener doesn't have local address")
    }

    fn ttl(&self) -> io::Result<u32> {
        TcpListener::ttl(self)
    }

    fn set_ttl(&self, ttl: u32) -> io::Result<()> {
        TcpListener::set_ttl(self, ttl)
    }

    fn set_nonblocking(&self, nonblocking: bool) -> io::Result<()> {
        TcpListener::set_nonblocking(self, nonblocking)
    }

    fn try_clone(&self) -> io::Result<Self>
    where
        Self: Sized,
    {
        TcpListener::try_clone(self)
    }

    fn take_error(&self) -> io::Result<Option<io::Error>> {
        TcpListener::take_error(self)
    }
}

#[cfg(feature = "connect_nonblocking")]
impl NetListener for socket2::Socket {
    type Stream = socket2::Socket;

    fn bind(addr: &impl ToSocketAddrs) -> io::Result<Self>
    where
        Self: Sized,
    {
        let addr = addr
            .to_socket_addrs()?
            .next()
            .ok_or(io::ErrorKind::InvalidInput)?;
        let socket = socket2::Socket::new(
            socket2::Domain::for_address(addr),
            socket2::Type::STREAM,
            None,
        )?;
        socket2::Socket::bind(&socket, &addr.into())?;
        Ok(socket)
    }

    fn accept(&self) -> io::Result<Self::Stream> {
        Ok(socket2::Socket::accept(self)?.0)
    }

    fn local_addr(&self) -> SocketAddr {
        socket2::Socket::local_addr(self)
            .expect("TCP listener doesn't have local address")
            .as_socket()
            .expect("TCP listener doesn't has local socket")
    }

    fn ttl(&self) -> io::Result<u32> {
        socket2::Socket::ttl(self)
    }

    fn set_ttl(&self, ttl: u32) -> io::Result<()> {
        socket2::Socket::set_ttl(self, ttl)
    }

    fn set_nonblocking(&self, nonblocking: bool) -> io::Result<()> {
        socket2::Socket::set_nonblocking(self, nonblocking)
    }

    fn try_clone(&self) -> io::Result<Self>
    where
        Self: Sized,
    {
        socket2::Socket::try_clone(self)
    }

    fn take_error(&self) -> io::Result<Option<io::Error>> {
        socket2::Socket::take_error(self)
    }
}
