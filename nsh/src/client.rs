use cyphernet::crypto::ed25519::PrivateKey;
use netservices::noise::NoiseXk;
use netservices::socks5::Socks5Error;
use std::net;

use crate::RemoteAddr;

pub type NetTransport = netservices::NetTransport<NoiseXk<PrivateKey>>;

pub struct Response {}

impl Iterator for Response {
    type Item = Vec<u8>;

    fn next(&mut self) -> Option<Self::Item> {
        todo!()
    }
}

pub struct Client {}

impl Client {
    pub fn connect(
        ecdh: &PrivateKey,
        remote_addr: RemoteAddr,
        //socks5_proxy: net::SocketAddr,
    ) -> Result<Self, Socks5Error> {
        todo!()
        //NetTransport::connect_via_socks5(remote_addr, ecdh, socks5_proxy)?;
    }

    pub fn exec(&mut self, command: String) -> Response {
        todo!()
    }

    pub fn disconnect(self) {}
}
