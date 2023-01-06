use std::fmt::{Debug, Display};
use std::hash::Hash;
use std::mem::MaybeUninit;
use std::net::{Shutdown, TcpStream};
use std::os::unix::io::AsRawFd;
use std::time::Duration;
use std::{io, net};

use cyphernet::addr::Addr;
use reactor::{ReadNonblocking, WriteNonblocking};

use crate::wire::{SplitIo, SplitIoError};

pub trait ResAddr: Addr + Copy + Ord + Eq + Hash + Debug + Display {}
impl<T> ResAddr for T where T: Addr + Copy + Ord + Eq + Hash + Debug + Display {}

pub trait StreamNonblocking: ReadNonblocking + WriteNonblocking {}

impl<T> StreamNonblocking for T where T: ReadNonblocking + WriteNonblocking {}

/// Network stream is an abstraction of TCP stream object.
pub trait NetConnection: Send + SplitIo + StreamNonblocking + AsRawFd {
    type Addr: ResAddr + Send;

    fn connect_nonblocking(addr: Self::Addr) -> io::Result<Self>
    where
        Self: Sized;

    fn shutdown(&mut self, how: Shutdown) -> io::Result<()>;

    fn remote_addr(&self) -> Self::Addr;
    fn local_addr(&self) -> Self::Addr;

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

impl SplitIo for TcpStream {
    type Read = Self;
    type Write = Self;
    type Err = io::Error;

    fn split_io(self) -> Result<(Self::Read, Self::Write), SplitIoError<Self>> {
        match self.try_clone() {
            Ok(clone) => Ok((clone, self)),
            Err(error) => Err(SplitIoError {
                original: self,
                error,
            }),
        }
    }

    fn from_split_io(read: Self::Read, write: Self::Write) -> Self {
        // TODO: Do a better detection of unrelated join
        if read.as_raw_fd() != write.as_raw_fd() {
            panic!("attempt to join unrelated streams")
        }
        read
    }
}

impl NetConnection for TcpStream {
    type Addr = net::SocketAddr;

    #[cfg(not(feature = "socket2"))]
    fn connect_nonblocking(addr: Self::Addr) -> io::Result<Self> {
        panic!("non-blocking TcpStream::connect requires socket2 feature")
    }
    #[cfg(feature = "socket2")]
    fn connect_nonblocking(addr: Self::Addr) -> io::Result<Self> {
        <socket2::Socket as NetConnection>::connect_nonblocking(addr).map(Self::from)
    }

    fn shutdown(&mut self, how: Shutdown) -> io::Result<()> {
        TcpStream::shutdown(self, how)
    }

    fn remote_addr(&self) -> Self::Addr {
        TcpStream::peer_addr(self).expect("TCP stream doesn't know remote peer address")
    }

    fn local_addr(&self) -> Self::Addr {
        TcpStream::local_addr(self).expect("TCP stream doesn't has local address")
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
impl NetConnection for socket2::Socket {
    type Addr = net::SocketAddr;

    fn connect_nonblocking(addr: Self::Addr) -> io::Result<Self> {
        let addr = addr.into();
        let socket = socket2::Socket::new(
            socket2::Domain::for_address(addr),
            socket2::Type::STREAM,
            None,
        )?;
        socket.set_nonblocking(true)?;
        match socket2::Socket::connect(&socket, &addr.into()) {
            Ok(()) => {}
            Err(e) if e.raw_os_error() == Some(libc::EINPROGRESS) => {}
            Err(e) if e.raw_os_error() == Some(libc::EALREADY) => {
                return Err(io::Error::from(io::ErrorKind::AlreadyExists))
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {}
            Err(e) => return Err(e),
        }
        Ok(socket)
    }

    fn shutdown(&mut self, how: Shutdown) -> io::Result<()> {
        socket2::Socket::shutdown(self, how)
    }

    fn remote_addr(&self) -> Self::Addr {
        socket2::Socket::peer_addr(self)
            .expect("net stream must use only connections")
            .as_socket()
            .expect("net stream must use only connections")
    }

    fn local_addr(&self) -> Self::Addr {
        socket2::Socket::local_addr(self)
            .expect("net stream doesn't has local socket")
            .as_socket()
            .expect("net stream doesn't has local socket")
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

#[cfg(feature = "socket2")]
impl SplitIo for socket2::Socket {
    type Read = Self;
    type Write = Self;
    type Err = io::Error;

    fn split_io(self) -> Result<(Self::Read, Self::Write), SplitIoError<Self>> {
        match self.try_clone() {
            Ok(clone) => Ok((clone, self)),
            Err(error) => Err(SplitIoError {
                original: self,
                error,
            }),
        }
    }

    fn from_split_io(read: Self::Read, write: Self::Write) -> Self {
        // TODO: Do a better detection of unrelated join
        if read.as_raw_fd() != write.as_raw_fd() {
            panic!("attempt to join unrelated streams")
        }
        read
    }
}
