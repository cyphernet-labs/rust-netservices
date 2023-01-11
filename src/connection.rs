use std::fmt::{Debug, Display};
use std::hash::Hash;
use std::mem::MaybeUninit;
use std::net::{Shutdown, TcpStream, ToSocketAddrs};
use std::os::unix::io::AsRawFd;
use std::time::Duration;
use std::{io, net};

use cyphernet::addr::{Addr, HostName, NetAddr};

use crate::socks5::ToSocks5Dst;
use crate::{SplitIo, SplitIoError};

pub trait Address: Addr + Clone + Eq + Hash + Debug + Display {}
impl<T> Address for T where T: Addr + Clone + Eq + Hash + Debug + Display {}

pub trait Proxy: ToSocketAddrs {
    type Error: std::error::Error + From<io::Error>;

    fn connect_blocking<A: ToSocks5Dst>(&self, addr: A) -> Result<TcpStream, Self::Error>;

    #[cfg(feature = "socket2")]
    fn connect_nonblocking<A: ToSocks5Dst>(&self, addr: A) -> Result<TcpStream, Self::Error>;
}

/// Network stream is an abstraction of TCP stream object.
pub trait NetConnection: Send + SplitIo + io::Read + io::Write + AsRawFd + Debug {
    type Addr: Address + Send;

    fn connect_blocking<P: Proxy>(addr: Self::Addr, proxy: &P) -> Result<Self, P::Error>
    where
        Self: Sized;

    #[cfg(feature = "socket2")]
    fn connect_nonblocking<P: Proxy>(addr: Self::Addr, proxy: &P) -> Result<Self, P::Error>
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

impl NetConnection for TcpStream {
    type Addr = NetAddr<HostName>;

    fn connect_blocking<P: Proxy>(addr: Self::Addr, proxy: &P) -> Result<Self, P::Error> {
        match addr.host {
            HostName::Ip(ip) => TcpStream::connect((ip, addr.port)).map_err(P::Error::from),
            _ => proxy.connect_blocking(addr),
        }
    }

    #[cfg(feature = "socket2")]
    fn connect_nonblocking<P: Proxy>(addr: Self::Addr, proxy: &P) -> Result<Self, P::Error> {
        Ok(socket2::Socket::connect_nonblocking(addr, proxy)?.into())
    }

    fn shutdown(&mut self, how: Shutdown) -> io::Result<()> {
        TcpStream::shutdown(self, how)
    }

    fn remote_addr(&self) -> Self::Addr {
        TcpStream::peer_addr(self)
            .expect("TCP stream doesn't know remote peer address")
            .into()
    }

    fn local_addr(&self) -> Self::Addr {
        TcpStream::local_addr(self)
            .expect("TCP stream doesn't has local address")
            .into()
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
    type Addr = NetAddr<HostName>;

    fn connect_blocking<P: Proxy>(addr: Self::Addr, proxy: &P) -> Result<Self, P::Error> {
        Ok(match addr.host {
            HostName::Ip(ip) => TcpStream::connect((ip, addr.port))?,
            _ => proxy.connect_blocking(addr)?,
        }
        .into())
    }

    fn connect_nonblocking<P: Proxy>(addr: Self::Addr, proxy: &P) -> Result<Self, P::Error> {
        match addr.host {
            HostName::Ip(ip) => {
                let addr = net::SocketAddr::new(ip, addr.port);
                let socket = socket2::Socket::new(
                    socket2::Domain::for_address(addr),
                    socket2::Type::STREAM,
                    None,
                )?;
                socket.set_nonblocking(true)?;
                match socket2::Socket::connect(&socket, &addr.into()) {
                    Ok(()) => {
                        #[cfg(feature = "log")]
                        log::debug!(target: "netservices", "Connected to {}", addr);
                    }
                    Err(e) if e.raw_os_error() == Some(libc::EINPROGRESS) => {
                        #[cfg(feature = "log")]
                        log::debug!(target: "netservices", "Connecting to {} in a non-blocking way", addr);
                    }
                    Err(e) if e.raw_os_error() == Some(libc::EALREADY) => {
                        #[cfg(feature = "log")]
                        log::error!(target: "netservices", "Can't connect to {}: address already in use", addr);
                        return Err(io::Error::from(io::ErrorKind::AlreadyExists).into());
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                        #[cfg(feature = "log")]
                        log::error!(target: "netservices", "Can't connect to {} in a non-blocking way", addr);
                    }
                    Err(e) => {
                        #[cfg(feature = "log")]
                        log::debug!(target: "netservices", "Error connecting to {}: {}", addr, e);
                        return Err(e.into());
                    }
                }
                Ok(socket)
            }
            _ => Ok(proxy.connect_blocking(addr)?.into()),
        }
    }

    fn shutdown(&mut self, how: Shutdown) -> io::Result<()> {
        socket2::Socket::shutdown(self, how)
    }

    fn remote_addr(&self) -> Self::Addr {
        socket2::Socket::peer_addr(self)
            .expect("net stream must use only connections")
            .as_socket()
            .expect("net stream must use only connections")
            .into()
    }

    fn local_addr(&self) -> Self::Addr {
        socket2::Socket::local_addr(self)
            .expect("net stream doesn't has local socket")
            .as_socket()
            .expect("net stream doesn't has local socket")
            .into()
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

impl SplitIo for TcpStream {
    type Read = Self;
    type Write = Self;

    fn split_io(self) -> Result<(Self::Read, Self::Write), SplitIoError<Self>> {
        match self.try_clone() {
            Ok(clone) => Ok((clone, self)),
            Err(error) => Err(SplitIoError {
                original: self,
                error,
            }),
        }
    }

    fn from_split_io(_read: Self::Read, write: Self::Write) -> Self {
        write
    }
}

#[cfg(feature = "socket2")]
impl SplitIo for socket2::Socket {
    type Read = Self;
    type Write = Self;

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
