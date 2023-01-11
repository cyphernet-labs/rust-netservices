#[macro_use]
extern crate amplify;
extern crate log_crate as log;

mod auth;
mod connection;
mod encryption;
mod proxy;

mod base;
pub mod frame;
pub mod tunnel;

#[cfg(feature = "re-actor")]
pub mod actors;
#[cfg(feature = "io-reactor")]
pub mod reactor;

pub use auth::Authenticator;
pub use base::{NetReader, NetWriter, SplitIo, SplitIoError};
pub use connection::{Address, NetConnection, NetListener, Proxy};
pub use frame::{Frame, Marshaller};

#[cfg(feature = "io-reactor")]
pub use reactor::{ListenerEvent, NetAccept, NetTransport, SessionEvent};

pub trait NetStream: std::io::Read + std::io::Write {}

pub trait NetSession: NetStream {
    /// Inner session type
    type Inner: NetSession;
    /// Underlying connection
    type Connection: NetConnection;
    type Artifact;

    fn is_established(&self) -> bool {
        self.artifact().is_some()
    }
    fn artifact(&self) -> Option<Self::Artifact>;
    fn as_connection(&self) -> &Self::Connection;
    fn disconnect(self) -> std::io::Result<()>;
}

mod imp_std {
    use std::net::{Shutdown, SocketAddr, TcpStream};

    use super::*;

    impl NetStream for TcpStream {}
    impl NetSession for TcpStream {
        type Inner = ();
        type Connection = Self;
        type Artifact = SocketAddr;

        fn artifact(&self) -> Option<Self::Artifact> {
            self.peer_addr().ok()
        }

        fn as_connection(&self) -> &Self::Connection {
            self
        }

        fn disconnect(self) -> std::io::Result<()> {
            self.shutdown(Shutdown::Both)
        }
    }
}

#[cfg(feature = "socket2")]
mod imp_socket2 {
    use std::net::{Shutdown, SocketAddr};

    use socket2::Socket;

    use super::*;

    impl NetStream for Socket {}
    impl NetSession for Socket {
        type Inner = ();
        type Connection = Self;
        type Artifact = SocketAddr;

        fn artifact(&self) -> Option<Self::Artifact> {
            self.peer_addr().ok()?.as_socket()
        }

        fn as_connection(&self) -> &Self::Connection {
            self
        }

        fn disconnect(self) -> std::io::Result<()> {
            self.shutdown(Shutdown::Both)
        }
    }
}
