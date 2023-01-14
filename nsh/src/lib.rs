#[macro_use]
extern crate amplify;
#[macro_use]
extern crate clap;

use cyphernet::addr::{HostName, NetAddr, PeerAddr};
use cyphernet::{ed25519, x25519, Sha256};
use netservices::session::{Eidolon, Noise, Socks5};
use std::net::TcpStream;

pub mod client;
pub mod command;
pub mod processor;
pub mod server;
pub mod shell;

pub type RemoteAddr = PeerAddr<ed25519::PublicKey, NetAddr<HostName>>;
pub type Session =
    Eidolon<ed25519::PrivateKey, Noise<x25519::PrivateKey, Sha256, Socks5<TcpStream>>>;
pub type Transport = netservices::NetTransport<Session>;

pub trait SessionBuilder {
    fn build() -> Self;
}

impl SessionBuilder for Session {
    fn build() -> Self {
        todo!()
    }
}
