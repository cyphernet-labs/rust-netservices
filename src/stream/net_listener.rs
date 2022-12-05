use std::io;
use std::net::{SocketAddr, ToSocketAddrs};
use std::os::unix::io::AsRawFd;

use crate::stream::NetStream;

pub trait NetListener: AsRawFd {
    type Stream: NetStream;

    fn bind<A: ToSocketAddrs>(addr: A) -> Result<Self, io::Error>
    where
        Self: Sized;

    fn accept(&self) -> Result<(Self::Stream, SocketAddr), io::Error>;

    fn local_addr(&self) -> SocketAddr;

    fn ttl(&self) -> Result<u32, io::Error>;
    fn set_nonblocking(&self, nonblocking: bool) -> Result<(), io::Error>;
    fn set_ttl(&self, ttl: u32) -> Result<(), io::Error>;
}
