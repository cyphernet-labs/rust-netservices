//! A library used for processing streams with filters organized into pipes.
//! Filters may perform tasks like multiplexing/demultiplexing, encrypting/
//! decrypting, encoding/decoding stream data, and conversions of the stream
//! into packets.

pub mod channel;
pub mod frame;
pub mod stream;
pub mod transcode;

pub use stream::NetStream;

/// Trait for disconnect reasons which must handle on-demand disconnections
pub trait OnDemand {
    /// Constructs variant for disconnect reason which indicates on-demand
    /// disconnection.
    fn on_demand() -> Self;
}

/// Address of a [`Resource`].
pub trait ResourceAddr: Clone + Eq + Send {
    type Raw: ResourceAddr;

    fn to_raw(&self) -> Self::Raw;

    fn raw_connection<R: Resource<Addr = Self>>(&self) -> Result<R::Raw, R::Error> {
        R::raw_connection(self)
    }
}

/// Any resource (network, local socket, file etc) which can be connected to
/// or disconnected to and operates I/O.
///
/// The resources can be composed; for this reason they expose underlying raw
/// resource via associated type [`Self::Raw`]. The connection to the resource
/// always happens through raw resource in [`Self::raw_connection`]. Composed
/// resources uses it to run more complex protocols for operations with I/O
/// streams (like encoding, framing etc) by providing higher-level constructors
/// or using resource timeout running handshake protocols.
pub trait Resource: std::io::Read + std::io::Write {
    /// Address type used by this resource
    type Addr: ResourceAddr<Raw = <Self::Raw as Resource>::Addr>;

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

    /// Connects the resource with the underlying raw protocol (i.e. without
    /// handshake).
    fn raw_connection(addr: &Self::Addr) -> Result<Self::Raw, Self::Error>
    where
        Self: Sized;

    /// Disconnects the resource.
    fn disconnect(&mut self) -> Result<(), Self::Error>;
}

impl ResourceAddr for std::net::SocketAddr {
    type Raw = Self;

    fn to_raw(&self) -> Self::Raw {
        *self
    }
}

impl ResourceAddr for std::net::SocketAddrV4 {
    type Raw = Self;

    fn to_raw(&self) -> Self::Raw {
        *self
    }
}

impl ResourceAddr for std::net::SocketAddrV6 {
    type Raw = Self;

    fn to_raw(&self) -> Self::Raw {
        *self
    }
}

impl ResourceAddr for std::net::IpAddr {
    type Raw = Self;

    fn to_raw(&self) -> Self::Raw {
        *self
    }
}

impl ResourceAddr for std::net::Ipv4Addr {
    type Raw = Self;

    fn to_raw(&self) -> Self::Raw {
        *self
    }
}

impl ResourceAddr for std::net::Ipv6Addr {
    type Raw = Self;

    fn to_raw(&self) -> Self::Raw {
        *self
    }
}

impl ResourceAddr for std::path::PathBuf {
    type Raw = Self;

    fn to_raw(&self) -> Self::Raw {
        self.clone()
    }
}

impl ResourceAddr for String {
    type Raw = Self;

    fn to_raw(&self) -> Self::Raw {
        self.clone()
    }
}
