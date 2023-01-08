#[macro_use]
extern crate amplify;

use cyphernet::addr::PeerAddr;
use cyphernet::crypto::ed25519::{PrivateKey, PublicKey};
use netservices::noise::NoiseXk;
use std::net;

pub mod client;
pub mod rsh;
pub mod server;
pub mod shell;

pub type RemoteAddr = PeerAddr<PublicKey, net::SocketAddr>;
pub type Transport = netservices::NetTransport<NoiseXk<PrivateKey>>;
