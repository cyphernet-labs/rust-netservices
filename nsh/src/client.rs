use cyphernet::crypto::ed25519::PrivateKey;
use netservices::noise::NoiseXk;
use netservices::socks5::Socks5Error;
use netservices::tunnel::READ_BUFFER_SIZE;
use netservices::NetSession;
use std::io::{self, Read, Write};

use crate::RemoteAddr;

pub type NetTransport = netservices::NetTransport<NoiseXk<PrivateKey>>;

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
    session: NetTransport,
}

impl Client {
    pub fn connect(
        ecdh: &PrivateKey,
        remote_addr: RemoteAddr,
        // socks5_proxy: net::SocketAddr,
    ) -> Result<Self, Socks5Error> {
        // TODO: Do socks5 connection
        let session = NetTransport::connect(remote_addr.into_remote_addr(), ecdh)?;
        Ok(Self {
            buf: vec![0u8; READ_BUFFER_SIZE],
            session,
        })
    }

    pub fn exec(mut self, command: String) -> io::Result<Response> {
        self.session.write_all(command.as_bytes())?;
        Ok(Response { client: self })
    }

    fn recv(&mut self) -> Option<Vec<u8>> {
        return match self.session.read(&mut self.buf) {
            Ok(len) => Some(self.buf[..len].to_vec()),
            Err(err) if err.kind() == io::ErrorKind::ConnectionReset => None,
            Err(err) => None,
        };
    }

    pub fn disconnect(self) -> io::Result<()> {
        self.session.disconnect()
    }
}
