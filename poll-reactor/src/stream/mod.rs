mod frame;
mod net_listener;
mod net_stream;

pub use frame::{Frame, VecFrame};
pub use net_listener::NetListener;
pub use net_stream::NetStream;

pub trait Stream: std::io::Write + std::io::Read {}

impl<T> Stream for T where T: std::io::Write + std::io::Read {}
