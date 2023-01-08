use std::io::{self, Read, Write};

use amplify::hex::ToHex;
use cyphernet::crypto::ed25519::PrivateKey;
use netservices::noise::NoiseXk;
use netservices::socks5::Socks5Error;
use netservices::tunnel::READ_BUFFER_SIZE;
use netservices::NetSession;

use crate::RemoteAddr;

pub type Transport = netservices::NetTransport<NoiseXk<PrivateKey>>;

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
    transport: Transport,
}

impl Client {
    pub fn connect(
        ecdh: &PrivateKey,
        remote_addr: RemoteAddr,
        // socks5_proxy: net::SocketAddr,
    ) -> Result<Self, Socks5Error> {
        // TODO: Do socks5 connection
        let session = Transport::connect(remote_addr, ecdh, false)?;
        Ok(Self {
            buf: vec![0u8; READ_BUFFER_SIZE],
            transport: session,
        })
    }

    pub fn exec(mut self, command: String) -> io::Result<Response> {
        log::debug!(target: "nsh", "Sending command '{}'", command);
        self.transport.write_all(command.as_bytes())?;
        self.transport.flush()?;
        Ok(Response { client: self })
    }

    fn recv(&mut self) -> Option<Vec<u8>> {
        return match self.transport.read(&mut self.buf) {
            Ok(len) => {
                log::trace!(target: "nsh", "Got reply from the remote: {}", self.buf[..len].to_hex());
                Some(self.buf[..len].to_vec())
            }
            Err(err) if err.kind() == io::ErrorKind::ConnectionReset => None,
            Err(err) => None,
        };
    }

    pub fn disconnect(self) -> io::Result<()> {
        self.transport.disconnect()
    }
}
