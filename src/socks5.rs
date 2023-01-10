use std::net::{SocketAddr, TcpStream, ToSocketAddrs};
use std::{io, net, option};

use cyphernet::addr::{Host, NetAddr};

use crate::connection::Proxy;

#[derive(Debug, Display, Error, From)]
#[display(doc_comments)]
pub enum Socks5Error {
    #[from]
    #[from(io::ErrorKind)]
    #[display(inner)]
    Io(io::Error),
}

pub trait ToSocks5Dst {}

impl ToSocks5Dst for String {}

impl ToSocks5Dst for net::SocketAddr {}
impl ToSocks5Dst for net::SocketAddrV4 {}
impl ToSocks5Dst for net::SocketAddrV6 {}
impl<H: Host> ToSocks5Dst for NetAddr<H> {}

#[derive(Clone, Eq, PartialEq, Debug)]
pub struct Socks5 {
    proxy: SocketAddr,
}

impl Socks5 {
    pub fn new(proxy_addr: impl ToSocketAddrs) -> io::Result<Self> {
        Ok(Self {
            proxy: proxy_addr
                .to_socket_addrs()?
                .next()
                .ok_or_else(|| io::ErrorKind::InvalidInput)?,
        })
    }
}

impl ToSocketAddrs for Socks5 {
    type Iter = option::IntoIter<SocketAddr>;

    fn to_socket_addrs(&self) -> io::Result<Self::Iter> {
        Ok(Some(self.proxy).into_iter())
    }
}

impl Proxy for Socks5 {
    type Error = Socks5Error;

    fn connect_blocking<A: ToSocks5Dst>(&self, addr: A) -> Result<TcpStream, Self::Error> {
        todo!()
    }

    fn connect_nonblocking<A: ToSocks5Dst>(&self, addr: A) -> Result<TcpStream, Self::Error> {
        todo!()
    }
}
