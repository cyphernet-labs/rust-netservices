//! A library used for processing streams with filters organized into pipes.
//! Filters may perform tasks like multiplexing/demultiplexing, encrypting/
//! decrypting, encoding/decoding stream data, and conversions of the stream
//! into packets.

pub mod channel;
pub mod frame;
pub mod stream;
pub mod transcode;

use std::io;
use std::net::SocketAddr;
pub use stream::NetStream;

/// Trait for disconnect reasons which must handle on-demand disconnections
pub trait OnDemand {
    /// Constructs variant for disconnect reason which indicates on-demand
    /// disconnection.
    fn on_demand() -> Self;
}

/// Address of a resource
pub trait ResourceAddr: Eq + Clone + Send {
    type Raw: ResourceAddr;

    fn to_raw(&self) -> Self::Raw;

    fn raw_connection<R: Resource<Addr = Self>>(&self) -> Result<R::Raw, R::Error> {
        R::raw_connection(self)
    }
}

/// Specific resource (network or local) monitored by the reactor for the
/// I/O events.
pub trait Resource: io::Read + io::Write {
    /// Address type used by this resource
    type Addr: ResourceAddr;

    /// Underlying raw resource type
    type Raw: Resource;

    /// Reasons of resource disconnects
    type DisconnectReason: OnDemand;

    /// Error type for resource connectivity.
    type Error;

    /// Returns an address of the resource.
    fn addr(&self) -> Self::Addr;

    /// Returns address of the underlying raw resource.
    fn raw_addr(&self) -> <Self::Raw as Resource>::Addr;

    /// Connects the resource with the underlying raw protocol
    /// (i.e. without handshake).
    fn raw_connection(addr: &Self::Addr) -> Result<Self::Raw, Self::Error>
    where
        Self: Sized;

    /// Disconnects the resource.
    fn disconnect(&mut self) -> Result<(), Self::Error>;
}

impl ResourceAddr for SocketAddr {
    type Raw = Self;

    fn to_raw(&self) -> Self::Raw {
        *self
    }
}
