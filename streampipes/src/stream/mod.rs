// mod filtered;
mod net_stream;

pub use net_stream::NetStream;

pub trait Stream: std::io::Write + std::io::Read {}

impl<T> Stream for T where T: std::io::Write + std::io::Read {}
