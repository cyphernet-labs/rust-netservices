#[macro_use]
extern crate amplify;
#[macro_use]
extern crate clap;

use cyphernet::addr::{HostName, NetAddr, PeerAddr};
use cyphernet::crypto::ed25519::{PrivateKey, PublicKey};
use netservices::noise::NoiseXk;

pub mod client;
pub mod command;
pub mod processor;
pub mod server;
pub mod shell;

pub type RemoteAddr = PeerAddr<PublicKey, NetAddr<HostName>>;
pub type Transport = netservices::NetResource<NoiseXk<PrivateKey>>;
