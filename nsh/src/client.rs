use std::io::{self, Read, Write};

use amplify::hex::ToHex;
use cyphernet::crypto::ed25519::PrivateKey;
use netservices::noise::NoiseXk;
use netservices::tunnel::READ_BUFFER_SIZE;
use netservices::{NetSession, Proxy};

use crate::command::Command;
use crate::RemoteAddr;

pub type Session = NoiseXk<PrivateKey>;

pub struct Response {
    client: Client,
}

impl Response {
    pub fn complete(self) -> Client {
        self.client
    }
}

impl<'a> Iterator for &'a mut Response {
    type Item = Vec<u8>;

    fn next(&mut self) -> Option<Self::Item> {
        self.client.recv()
    }
}

pub struct Client {
    buf: Vec<u8>,
    session: Session,
}

impl Client {
    pub fn connect<P: Proxy>(
        ecdh: &PrivateKey,
        remote_addr: RemoteAddr,
        proxy: &P,
    ) -> Result<Self, P::Error> {
        let session = Session::connect_blocking(remote_addr, ecdh, proxy)?;
        Ok(Self {
            buf: vec![0u8; READ_BUFFER_SIZE],
            session,
        })
    }

    pub fn exec(mut self, command: Command) -> io::Result<Response> {
        log::debug!(target: "nsh", "Sending command '{}'", command);
        self.session.write_all(command.to_string().as_bytes())?;
        self.session.flush()?;
        Ok(Response { client: self })
    }

    fn recv(&mut self) -> Option<Vec<u8>> {
        return match self.session.read(&mut self.buf) {
            Ok(0) => {
                log::warn!(target: "nsh", "Connection reset by {}", self.session.expect_id());
                None
            }
            Err(err) if err.kind() == io::ErrorKind::ConnectionReset => {
                log::warn!(target: "nsh", "Connection reset by {}", self.session.expect_id());
                None
            }
            Ok(len) => {
                log::trace!(target: "nsh", "Got reply from the remote: {}", self.buf[..len].to_hex());
                Some(self.buf[..len].to_vec())
            }
            Err(err) => {
                log::error!(target: "nsh", "Error from the remote: {err}");
                None
            }
        };
    }

    pub fn disconnect(self) -> io::Result<()> {
        self.session.disconnect()
    }
}
