use cyphernet::addr::Addr;
use std::io;
use std::net::TcpStream;

use crate::stream::NetStream;

pub trait NetSession: NetStream + Sized {
    type Context;
    type Inner: NetStream;
    type RemoteAddr: Addr + Clone;

    fn accept(stream: Self::Inner, context: &Self::Context) -> Self;
    fn connect(addr: Self::RemoteAddr, context: &Self::Context) -> io::Result<Self>;
}

#[cfg(feature = "socket2")]
impl NetSession for TcpStream {
    type Context = ();
    type Inner = Self;
    type RemoteAddr = Self::Addr;

    fn accept(stream: Self::Inner, _context: &Self::Context) -> Self {
        stream
    }

    fn connect(addr: Self::RemoteAddr, context: &Self::Context) -> io::Result<Self> {
        <socket2::Socket as NetSession>::connect(addr, context).map(Self::from)
    }
}

#[cfg(feature = "socket2")]
impl NetSession for socket2::Socket {
    type Context = ();
    type Inner = Self;
    type RemoteAddr = Self::Addr;

    fn accept(stream: Self::Inner, _context: &Self::Context) -> Self {
        stream
    }

    fn connect(addr: Self::RemoteAddr, _context: &Self::Context) -> io::Result<Self> {
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
}
