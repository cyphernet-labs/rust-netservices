#[macro_use]
extern crate amplify;

#[cfg(feature = "re-actor")]
pub mod actors;

#[cfg(feature = "io-reactor")]
pub mod wire;

mod connection;
mod frame;
mod listener;
pub mod noise;
mod session;
mod tunnel;

pub use connection::{NetConnection, ResAddr};
pub use frame::{Frame, Marshaller};
pub use listener::NetListener;
pub use session::NetSession;
#[cfg(feature = "io-reactor")]
pub use wire::{ListenerEvent, NetAccept, NetTransport, SessionEvent};

pub trait IoStream: std::io::Write + std::io::Read {}

impl<T> IoStream for T where T: std::io::Write + std::io::Read {}
