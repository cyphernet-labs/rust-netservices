//! A library used for processing streams with filters organized into pipes.
//! Filters may perform tasks like multiplexing/demultiplexing, encrypting/
//! decrypting, encoding/decoding stream data, and conversions of the stream
//! into packets.

pub mod frame;
mod stream;

pub use stream::{NetStream, Stream};
