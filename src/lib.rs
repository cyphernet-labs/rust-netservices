#[macro_use]
extern crate amplify;
extern crate log_crate as log;

pub mod frame;
pub mod tunnel;

mod connection;
mod listener;
pub mod session;
mod split;

#[cfg(feature = "io-reactor")]
pub mod resource;

pub use connection::{Address, NetConnection, NetStream};
pub use frame::{Frame, Marshaller};
pub use listener::NetListener;
pub use session::{NetProtocol, NetSession, NetStateMachine};
pub use split::{NetReader, NetWriter, SplitIo, SplitIoError, TcpReader, TcpWriter};

#[cfg(feature = "io-reactor")]
pub use resource::{ListenerEvent, NetAccept, NetTransport, SessionEvent};

#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub enum LinkDirection {
    Inbound,
    Outbound,
}

impl LinkDirection {
    pub fn is_inbound(self) -> bool {
        matches!(self, LinkDirection::Inbound)
    }
    pub fn is_outbound(self) -> bool {
        matches!(self, LinkDirection::Outbound)
    }
}
