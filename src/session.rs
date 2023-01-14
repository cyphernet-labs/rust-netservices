use std::fmt::Debug;
use std::io;

use crate::{NetConnection, NetStream};

pub trait NetSession: NetStream + Debug {
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
    fn as_connection_mut(&mut self) -> &mut Self::Connection;
    fn disconnect(self) -> io::Result<()>;
}

pub trait NetStateMachine: Debug {
    type Artifact;
    type Error: std::error::Error + Send + Sync + 'static;

    fn next_read_len(&self) -> usize;
    fn advance(&mut self, input: &[u8]) -> Result<Vec<u8>, Self::Error>;
    fn artifact(&self) -> Option<Self::Artifact>;
    fn is_complete(&self) -> bool {
        self.artifact().is_some()
    }
}

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub struct NetProtocol<M: NetStateMachine, S: NetSession> {
    name: &'static str,
    state_machine: M,
    session: S,
}

impl<M: NetStateMachine, S: NetSession> NetProtocol<M, S> {
    pub fn new(name: &'static str, session: S, state_machine: M) -> Self {
        Self {
            name,
            state_machine,
            session,
        }
    }
}

impl<M: NetStateMachine, S: NetSession> io::Read for NetProtocol<M, S> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.state_machine.is_complete() {
            return self.session.read(buf);
        }

        let len = self.state_machine.next_read_len();
        let mut input = vec![0u8; len];
        self.session.read_exact(&mut input)?;

        #[cfg(feature = "log")]
        log::trace!(target: self.name, "Received handshake act: {input:02x?}");

        if !input.is_empty() {
            let output = self
                .state_machine
                .advance(&input)
                .map_err(|err| io::Error::new(io::ErrorKind::ConnectionAborted, err))?;

            #[cfg(feature = "log")]
            log::trace!(target: self.name, "Sending handshake act: {output:02x?}");

            if !output.is_empty() {
                self.session.write_all(&output)?;
            }
        }
        Ok(0)
    }
}

impl<M: NetStateMachine, S: NetSession> io::Write for NetProtocol<M, S> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.state_machine.is_complete() {
            return self.session.write(buf);
        }
        let act = self
            .state_machine
            .advance(&[])
            .map_err(|err| io::Error::new(io::ErrorKind::ConnectionAborted, err))?;

        if !act.is_empty() {
            #[cfg(feature = "log")]
            log::trace!(target: self.name, "Initializing handshake with: {act:02x?}");

            self.session.write_all(&act)?;

            Err(io::ErrorKind::Interrupted.into())
        } else {
            self.session.write(buf)
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        self.session.flush()
    }
}

impl<M: NetStateMachine, S: NetSession> NetStream for NetProtocol<M, S> {}

impl<M: NetStateMachine, S: NetSession> NetSession for NetProtocol<M, S> {
    type Inner = S;
    type Connection = S::Connection;
    type Artifact = (S::Artifact, M::Artifact);

    fn artifact(&self) -> Option<Self::Artifact> {
        self.session
            .artifact()
            .and_then(|a| self.state_machine.artifact().map(|b| (a, b)))
    }

    fn as_connection(&self) -> &Self::Connection {
        self.session.as_connection()
    }

    fn as_connection_mut(&mut self) -> &mut Self::Connection {
        self.session.as_connection_mut()
    }

    fn disconnect(self) -> io::Result<()> {
        self.session.disconnect()
    }
}

mod imp_std {
    use std::net::{Shutdown, SocketAddr, TcpStream};

    use super::*;

    impl NetSession for TcpStream {
        type Inner = Self;
        type Connection = Self;
        type Artifact = SocketAddr;

        fn artifact(&self) -> Option<Self::Artifact> {
            self.peer_addr().ok()
        }

        fn as_connection(&self) -> &Self::Connection {
            self
        }

        fn as_connection_mut(&mut self) -> &mut Self::Connection {
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

    impl NetSession for Socket {
        type Inner = Self;
        type Connection = Self;
        type Artifact = SocketAddr;

        fn artifact(&self) -> Option<Self::Artifact> {
            self.peer_addr().ok()?.as_socket()
        }

        fn as_connection(&self) -> &Self::Connection {
            self
        }

        fn as_connection_mut(&mut self) -> &mut Self::Connection {
            self
        }

        fn disconnect(self) -> std::io::Result<()> {
            self.shutdown(Shutdown::Both)
        }
    }
}
