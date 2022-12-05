use std::mem::MaybeUninit;
use std::net::{Shutdown, TcpStream};
use std::os::unix::io::AsRawFd;
use std::time::Duration;
use std::{io, net};

use cyphernet::addr::Addr;

use super::Stream;

/// Network stream is an abstraction of TCP stream object.
pub trait NetStream: Stream + AsRawFd {
    type Addr: Addr + Clone;

    fn shutdown(&mut self, how: Shutdown) -> io::Result<()>;

    fn peer_addr(&self) -> io::Result<Self::Addr>;
    fn local_addr(&self) -> io::Result<Self::Addr>;

    fn set_read_timeout(&mut self, dur: Option<Duration>) -> io::Result<()>;
    fn set_write_timeout(&mut self, dur: Option<Duration>) -> io::Result<()>;
    fn read_timeout(&self) -> io::Result<Option<Duration>>;
    fn write_timeout(&self) -> io::Result<Option<Duration>>;

    fn peek(&self, buf: &mut [u8]) -> io::Result<usize>;

    fn set_nodelay(&mut self, nodelay: bool) -> io::Result<()>;
    fn nodelay(&self) -> io::Result<bool>;
    fn set_ttl(&mut self, ttl: u32) -> io::Result<()>;
    fn ttl(&self) -> io::Result<u32>;
    fn set_nonblocking(&mut self, nonblocking: bool) -> io::Result<()>;

    fn try_clone(&self) -> io::Result<Self>
    where
        Self: Sized;
    fn take_error(&self) -> io::Result<Option<io::Error>>;
}

impl NetStream for TcpStream {
    type Addr = net::SocketAddr;

    fn shutdown(&mut self, how: Shutdown) -> io::Result<()> {
        TcpStream::shutdown(self, how)
    }

    fn peer_addr(&self) -> io::Result<Self::Addr> {
        TcpStream::peer_addr(self)
    }

    fn local_addr(&self) -> io::Result<Self::Addr> {
        TcpStream::local_addr(self)
    }

    fn set_read_timeout(&mut self, dur: Option<Duration>) -> io::Result<()> {
        TcpStream::set_read_timeout(self, dur)
    }
    fn set_write_timeout(&mut self, dur: Option<Duration>) -> io::Result<()> {
        TcpStream::set_write_timeout(self, dur)
    }
    fn read_timeout(&self) -> io::Result<Option<Duration>> {
        TcpStream::read_timeout(self)
    }
    fn write_timeout(&self) -> io::Result<Option<Duration>> {
        TcpStream::write_timeout(self)
    }

    fn peek(&self, buf: &mut [u8]) -> io::Result<usize> {
        TcpStream::peek(self, buf)
    }

    fn set_nodelay(&mut self, nodelay: bool) -> io::Result<()> {
        TcpStream::set_nodelay(self, nodelay)
    }
    fn nodelay(&self) -> io::Result<bool> {
        TcpStream::nodelay(self)
    }
    fn set_ttl(&mut self, ttl: u32) -> io::Result<()> {
        TcpStream::set_ttl(self, ttl)
    }
    fn ttl(&self) -> io::Result<u32> {
        TcpStream::ttl(self)
    }
    fn set_nonblocking(&mut self, nonblocking: bool) -> io::Result<()> {
        TcpStream::set_nonblocking(self, nonblocking)
    }

    fn try_clone(&self) -> io::Result<TcpStream> {
        TcpStream::try_clone(self)
    }
    fn take_error(&self) -> io::Result<Option<io::Error>> {
        TcpStream::take_error(self)
    }
}

#[cfg(feature = "socket2")]
impl NetStream for socket2::Socket {
    type Addr = net::SocketAddr;

    fn shutdown(&mut self, how: Shutdown) -> io::Result<()> {
        socket2::Socket::shutdown(self, how)
    }

    fn peer_addr(&self) -> io::Result<Self::Addr> {
        Ok(socket2::Socket::peer_addr(self)?
            .as_socket()
            .expect("net stream must use only connections"))
    }

    fn local_addr(&self) -> io::Result<Self::Addr> {
        Ok(socket2::Socket::local_addr(self)?
            .as_socket()
            .expect("net stream doesn't has local socket"))
    }

    fn set_read_timeout(&mut self, dur: Option<Duration>) -> io::Result<()> {
        socket2::Socket::set_read_timeout(self, dur)
    }

    fn set_write_timeout(&mut self, dur: Option<Duration>) -> io::Result<()> {
        socket2::Socket::set_write_timeout(self, dur)
    }

    fn read_timeout(&self) -> io::Result<Option<Duration>> {
        socket2::Socket::read_timeout(self)
    }

    fn write_timeout(&self) -> io::Result<Option<Duration>> {
        socket2::Socket::write_timeout(self)
    }

    fn peek(&self, buf: &mut [u8]) -> io::Result<usize> {
        let mut buf2 = vec![MaybeUninit::<u8>::uninit(); buf.len()];
        let len = socket2::Socket::peek(self, &mut buf2)?;
        for i in 0..len {
            buf[i] = unsafe { buf2[i].assume_init() };
        }
        Ok(len)
    }

    fn set_nodelay(&mut self, nodelay: bool) -> io::Result<()> {
        socket2::Socket::set_nodelay(self, nodelay)
    }

    fn nodelay(&self) -> io::Result<bool> {
        socket2::Socket::nodelay(self)
    }

    fn set_ttl(&mut self, ttl: u32) -> io::Result<()> {
        socket2::Socket::set_ttl(self, ttl)
    }

    fn ttl(&self) -> io::Result<u32> {
        socket2::Socket::ttl(self)
    }

    fn set_nonblocking(&mut self, nonblocking: bool) -> io::Result<()> {
        socket2::Socket::set_nonblocking(self, nonblocking)
    }

    fn try_clone(&self) -> io::Result<Self> {
        socket2::Socket::try_clone(self)
    }

    fn take_error(&self) -> io::Result<Option<io::Error>> {
        socket2::Socket::take_error(self)
    }
}
