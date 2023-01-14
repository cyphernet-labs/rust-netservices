use std::io::{self, Read, Write};

use amplify::hex::ToHex;
use netservices::tunnel::READ_BUFFER_SIZE;
use netservices::NetSession;

use crate::command::Command;
use crate::{RemoteAddr, Session, SessionBuilder};

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
    pub fn connect(remote_addr: RemoteAddr) -> io::Result<Self> {
        // TODO: Construct session
        let session = Session::build();
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
                log::warn!(target: "nsh", "Connection reset by {}", self.session.display());
                None
            }
            Err(err) if err.kind() == io::ErrorKind::ConnectionReset => {
                log::warn!(target: "nsh", "Connection reset by {}", self.session.display());
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
