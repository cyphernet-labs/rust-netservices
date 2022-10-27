//! A library used for processing streams with filters organized into pipes.
//! Filters may perform tasks like multiplexing/demultiplexing, encrypting/
//! decrypting, encoding/decoding stream data, and conversions of the stream
//! into packets.

pub mod channel;
pub mod frame;
pub mod stream;
pub mod transcode;

pub use stream::NetStream;
