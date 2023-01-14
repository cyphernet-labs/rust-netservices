#[macro_use]
extern crate amplify;
extern crate log_crate as log;

pub mod frame;
pub mod tunnel;

mod connection;
mod listener;
pub mod session;

#[cfg(feature = "io-reactor")]
pub mod resource;

pub use connection::{Address, NetConnection, NetStream};
pub use frame::{Frame, Marshaller};
pub use listener::NetListener;
pub use session::{NetProtocol, NetSession, NetStateMachine};

#[cfg(feature = "io-reactor")]
pub use resource::{ListenerEvent, NetAccept, NetTransport, SessionEvent};
