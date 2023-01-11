#[macro_use]
extern crate amplify;
extern crate log_crate as log;

#[cfg(feature = "re-actor")]
pub mod actors;

#[cfg(feature = "io-reactor")]
pub mod resources;

mod auth;
mod connection;
mod frame;
mod listener;
pub mod noise;
mod session;
pub mod socks5;
mod transcoders;
pub mod tunnel;

pub use auth::Authenticator;
pub use connection::{Address, NetConnection, Proxy};
pub use frame::{Frame, Marshaller};
pub use listener::NetListener;
#[cfg(feature = "io-reactor")]
pub use resources::{ListenerEvent, NetAccept, NetResource, SessionEvent};
pub use session::NetSession;

#[derive(Debug, Display)]
#[display("{error}")]
pub struct SplitIoError<T: SplitIo> {
    pub original: T,
    pub error: std::io::Error,
}

impl<T: SplitIo + std::fmt::Debug> std::error::Error for SplitIoError<T> {}

pub trait SplitIo: Sized {
    type Read: std::io::Read + Sized;
    type Write: std::io::Write + Sized;

    /// # Panics
    ///
    /// If the split operation is not possible
    fn split_io(self) -> Result<(Self::Read, Self::Write), SplitIoError<Self>>;
    fn from_split_io(read: Self::Read, write: Self::Write) -> Self;
}
