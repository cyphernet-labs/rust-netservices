use cyphernet::addr::NetAddr;
use std::net::TcpStream;
use std::{io, net};

#[derive(Debug, Display, Error, From)]
#[display(doc_comments)]
pub enum Socks5Error {
    #[from]
    #[display(inner)]
    Io(io::Error),
}

pub trait ToSocks5Dst {}

impl ToSocks5Dst for String {}

impl ToSocks5Dst for net::SocketAddr {}
impl ToSocks5Dst for net::SocketAddrV4 {}
impl ToSocks5Dst for net::SocketAddrV6 {}
impl<const DEFAULT_PORT: u16> ToSocks5Dst for NetAddr<DEFAULT_PORT> {}

pub struct Socks5 {
    stream: TcpStream,
}

impl From<TcpStream> for Socks5 {
    fn from(stream: TcpStream) -> Self {
        Self { stream }
    }
}

impl Socks5 {
    pub fn connect(&mut self, addr: impl ToSocks5Dst) -> Result<TcpStream, Socks5Error> {
        todo!()
    }
}
