#[macro_use]
extern crate amplify;

use cyphernet::addr::{PeerAddr, UniversalAddr};
use cyphernet::crypto::ed25519::{PrivateKey, PublicKey};
use netservices::noise::NoiseXk;
use std::net;

pub mod client;
pub mod service;

pub type RemoteAddr = UniversalAddr<PeerAddr<PublicKey, net::SocketAddr>>;
pub type NetTransport = netservices::NetTransport<NoiseXk<PrivateKey>>;
