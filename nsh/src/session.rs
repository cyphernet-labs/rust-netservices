use std::net::TcpStream;

use cyphernet::addr::{HostName, NetAddr, PeerAddr};
use cyphernet::encrypt::noise::{HandshakePattern, Keyset, NoiseState};
use cyphernet::proxy::socks5;
use cyphernet::{ed25519, x25519, Cert, Digest, Sha256};
use netservices::session::{Eidolon, EidolonRuntime, Noise, Socks5};
use netservices::LinkDirection;

pub type RemoteAddr = PeerAddr<ed25519::PublicKey, NetAddr<HostName>>;
pub type Session =
    Eidolon<ed25519::PrivateKey, Noise<x25519::PrivateKey, Sha256, Socks5<TcpStream>>>;
pub type Transport = netservices::NetTransport<Session>;

#[derive(Clone, Debug)]
pub struct SessionConfig {
    pub cert: Cert<ed25519::Signature>,
    pub signer: ed25519::PrivateKey,
}

pub trait SessionBuild {
    fn build(
        connection: TcpStream,
        remote_addr: NetAddr<HostName>,
        direction: LinkDirection,
        config: SessionConfig,
    ) -> Self;
}

impl SessionBuild for Session {
    fn build(
        connection: TcpStream,
        remote_addr: NetAddr<HostName>,
        direction: LinkDirection,
        config: SessionConfig,
    ) -> Self {
        let SessionConfig { cert, signer } = config;

        let proxy = Socks5::with(connection, socks5::Socks5::with(remote_addr, false));
        let noise = NoiseState::initialize::<{ Sha256::OUTPUT_LEN }>(
            HandshakePattern::nn(),
            direction.is_outbound(),
            &[],
            Keyset::noise_nn(),
        );
        let encoding = Noise::with(proxy, noise);
        let eidolon = match direction {
            LinkDirection::Inbound => EidolonRuntime::initiator(signer, cert),
            LinkDirection::Outbound => EidolonRuntime::responder(signer, cert),
        };
        let auth = Eidolon::with(encoding, eidolon);

        auth
    }
}
