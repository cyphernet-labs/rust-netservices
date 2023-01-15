#[macro_use]
extern crate amplify;
#[macro_use]
extern crate clap;

pub mod client;
pub mod command;
pub mod processor;
pub mod server;
pub mod shell;

mod types {
    use cyphernet::addr::{HostName, NetAddr, PeerAddr};
    use cyphernet::{ed25519, Sha256};
    use netservices::session::CypherSession;

    pub type RemoteHost = PeerAddr<ed25519::PublicKey, NetAddr<HostName>>;
    pub type Session = CypherSession<ed25519::PrivateKey, Sha256>;
    pub type Transport = netservices::NetTransport<Session>;
}
pub use types::*;
