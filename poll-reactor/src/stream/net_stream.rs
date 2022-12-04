use std::io;
use std::net::{Shutdown, SocketAddr, TcpStream, ToSocketAddrs};
use std::os::unix::io::AsRawFd;
use std::time::Duration;

use super::Stream;

/// Network stream is an abstraction of TCP stream object.
pub trait NetStream: Stream + AsRawFd {
    type Addr: Into<SocketAddr>;
    type AddrList: ToSocketAddrs;
    type Inner: NetStream;

    fn connect(addr: Self::AddrList) -> io::Result<Self>
    where
        Self: Sized;
    fn connect_timeout(addr: &Self::Addr, timeout: Duration) -> io::Result<Self>
    where
        Self: Sized;
    fn shutdown(&mut self, how: Shutdown) -> io::Result<()> {
        unsafe { self.as_inner_mut().shutdown(how) }
    }

    fn peer_addr(&self) -> io::Result<Self::Addr>;
    fn local_addr(&self) -> io::Result<Self::Addr>;

    fn set_read_timeout(&mut self, dur: Option<Duration>) -> io::Result<()> {
        unsafe { self.as_inner_mut().set_read_timeout(dur) }
    }
    fn set_write_timeout(&mut self, dur: Option<Duration>) -> io::Result<()> {
        unsafe { self.as_inner_mut().set_write_timeout(dur) }
    }
    fn read_timeout(&self) -> io::Result<Option<Duration>> {
        unsafe { self.as_inner().read_timeout() }
    }
    fn write_timeout(&self) -> io::Result<Option<Duration>> {
        unsafe { self.as_inner().write_timeout() }
    }

    fn peek(&self, buf: &mut [u8]) -> io::Result<usize> {
        unsafe { self.as_inner().peek(buf) }
    }

    fn set_linger(&mut self, linger: Option<Duration>) -> io::Result<()> {
        unsafe { self.as_inner_mut().set_linger(linger) }
    }
    fn linger(&self) -> io::Result<Option<Duration>> {
        unsafe { self.as_inner().linger() }
    }
    fn set_nodelay(&mut self, nodelay: bool) -> io::Result<()> {
        unsafe { self.as_inner_mut().set_nodelay(nodelay) }
    }
    fn nodelay(&self) -> io::Result<bool> {
        unsafe { self.as_inner().nodelay() }
    }
    fn set_ttl(&mut self, ttl: u32) -> io::Result<()> {
        unsafe { self.as_inner_mut().set_ttl(ttl) }
    }
    fn ttl(&self) -> io::Result<u32> {
        unsafe { self.as_inner().ttl() }
    }
    fn set_nonblocking(&mut self, nonblocking: bool) -> io::Result<()> {
        unsafe { self.as_inner_mut().set_nonblocking(nonblocking) }
    }

    fn try_clone(&self) -> io::Result<TcpStream> {
        unsafe { self.as_inner().try_clone() }
    }
    fn take_error(&self) -> io::Result<Option<io::Error>> {
        unsafe { self.as_inner().take_error() }
    }

    #[doc(hidden)]
    unsafe fn as_inner(&self) -> &Self::Inner;
    #[doc(hidden)]
    unsafe fn as_inner_mut(&mut self) -> &mut Self::Inner;

    #[doc(hidden)]
    unsafe fn as_raw(&self) -> &TcpStream;
    #[doc(hidden)]
    unsafe fn as_raw_mut(&mut self) -> &mut TcpStream;
}

impl NetStream for TcpStream {
    type Addr = SocketAddr;
    type AddrList = SocketAddr;
    type Inner = TcpStream;

    fn connect(addr: Self::AddrList) -> io::Result<Self>
    where
        Self: Sized,
    {
        TcpStream::connect(addr)
    }
    fn connect_timeout(addr: &Self::Addr, timeout: Duration) -> io::Result<Self>
    where
        Self: Sized,
    {
        TcpStream::connect_timeout(addr, timeout)
    }

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

    #[allow(unstable_features, unstable_name_collisions)]
    fn set_linger(&mut self, linger: Option<Duration>) -> io::Result<()> {
        TcpStream::set_linger(self, linger)
    }
    #[allow(unstable_features, unstable_name_collisions)]
    fn linger(&self) -> io::Result<Option<Duration>> {
        TcpStream::linger(self)
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

    unsafe fn as_inner(&self) -> &Self::Inner {
        self
    }

    unsafe fn as_inner_mut(&mut self) -> &mut Self::Inner {
        self
    }

    unsafe fn as_raw(&self) -> &TcpStream {
        self
    }

    unsafe fn as_raw_mut(&mut self) -> &mut TcpStream {
        self
    }
}
