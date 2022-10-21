use std::net;

use crate::{Resource, ResourceAddr};

#[derive(Copy, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum TcpConnector {
    Listen(net::SocketAddr),
    Connect(net::SocketAddr),
}

impl ResourceAddr for TcpConnector {
    fn connect<R: Resource>(&self) -> Result<R, R::Error> {
        todo!()
    }
}

pub enum Tcp {
    Listener(net::TcpListener),
    Connection(net::TcpStream),
}

impl Resource for Tcp {
    type Addr = TcpConnector;
    type DisconnectReason = ();
    type Error = ();

    fn addr(&self) -> Self::Addr {
        match self {
            Tcp::Listener(listener) => TcpConnector::Listen(
                listener
                    .local_addr()
                    .expect("TCP must always know local address"),
            ),
            Tcp::Connection(stream) => TcpConnector::Connect(
                stream
                    .peer_addr()
                    .expect("TCP stream always has remote address"),
            ),
        }
    }

    fn connect(addr: Self::Addr) -> Result<Self, Self::Error>
    where
        Self: Sized,
    {
        todo!()
    }

    fn disconnect(&mut self) -> Result<(), Self::Error> {
        todo!()
    }
}
