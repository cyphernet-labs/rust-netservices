use cyphernet::auth::eidolon::EidolonState;
use std::fmt::{Debug, Display};
use std::io;

use crate::{NetConnection, NetStream};

pub type Eidolon<E, S> = NetProtocol<EidolonRuntime<E>, S>;

pub trait NetSession: NetStream {
    /// Inner session type
    type Inner: NetSession;
    /// Underlying connection
    type Connection: NetConnection;
    type Artifact: Display;

    fn is_established(&self) -> bool {
        self.artifact().is_some()
    }
    fn artifact(&self) -> Option<Self::Artifact>;
    fn as_connection(&self) -> &Self::Connection;
    fn as_connection_mut(&mut self) -> &mut Self::Connection;
    fn disconnect(self) -> io::Result<()>;
}

pub trait NetStateMachine {
    type Artifact;
    type Error: std::error::Error + Send + Sync + 'static;

    fn next_read_len(&self) -> usize;
    fn advance(&mut self, input: &[u8]) -> Result<Vec<u8>, Self::Error>;
    fn artifact(&self) -> Option<Self::Artifact>;
    fn is_complete(&self) -> bool {
        self.artifact().is_some()
    }
}

#[derive(Copy, Clone, Eq, PartialEq)]
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

#[derive(Clone, Eq, PartialEq, Hash, Debug, Display)]
#[display("{session}")]
pub struct ProtocolArtifact<M: NetStateMachine, S: NetSession> {
    pub session: S::Artifact,
    pub state: M::Artifact,
}

impl<M: NetStateMachine, S: NetSession> NetSession for NetProtocol<M, S> {
    type Inner = S;
    type Connection = S::Connection;
    type Artifact = ProtocolArtifact<M, S>;

    fn artifact(&self) -> Option<Self::Artifact> {
        Some(ProtocolArtifact {
            session: self.session.artifact()?,
            state: self.state_machine.artifact()?,
        })
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

        fn disconnect(self) -> io::Result<()> {
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

        fn disconnect(self) -> io::Result<()> {
            self.shutdown(Shutdown::Both)
        }
    }
}

mod imp_eidolon {
    use std::fmt::{self, Display, Formatter};

    use cyphernet::auth::eidolon;
    use cyphernet::display::{Encoding, MultiDisplay};
    use cyphernet::{Cert, CertFormat, EcSign};

    use super::*;

    pub struct EidolonRuntime<S: EcSign> {
        state: EidolonState<S::Sig>,
        signer: S,
    }

    impl<S: EcSign> Display for EidolonRuntime<S> {
        fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
            match self.state.remote_cert() {
                Some(cert) => {
                    f.write_str(&cert.display_fmt(&CertFormat::new(", ", Encoding::Base58)))
                }
                None => f.write_str("<unidentified>"),
            }
        }
    }

    impl<S: EcSign + Debug> NetStateMachine for EidolonRuntime<S> {
        type Artifact = Cert<S::Sig>;
        type Error = eidolon::Error;

        fn next_read_len(&self) -> usize {
            todo!()
        }

        fn advance(&mut self, input: &[u8]) -> Result<Vec<u8>, Self::Error> {
            self.state.advance(input, &self.signer)
        }

        fn artifact(&self) -> Option<Self::Artifact> {
            self.state.remote_cert().cloned()
        }
    }
}
pub use imp_eidolon::EidolonRuntime;
