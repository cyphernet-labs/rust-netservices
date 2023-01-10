#[macro_use]
extern crate amplify;
extern crate log_crate as log;

#[cfg(feature = "re-actor")]
pub mod actors;

#[cfg(feature = "io-reactor")]
pub mod resources;

mod connection;
mod frame;
mod listener;
pub mod noise;
mod session;
pub mod socks5;
pub mod tunnel;

pub use connection::{NetConnection, ResAddr};
pub use frame::{Frame, Marshaller};
pub use listener::NetListener;
#[cfg(feature = "io-reactor")]
pub use resources::{ListenerEvent, NetAccept, NetResource, SessionEvent};
pub use session::NetSession;
