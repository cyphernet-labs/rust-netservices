use std::io;
use std::os::unix::io::{AsRawFd, RawFd};

/// Information about I/O events which has happened for an actor
#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Debug)]
pub struct IoEv {
    /// Specifies whether I/O source has data to read.
    pub is_readable: bool,
    /// Specifies whether I/O source is ready for write operations.
    pub is_writable: bool,
}

pub trait Poll: Iterator<Item = (RawFd, IoEv)> {
    fn register(&mut self, fd: impl AsRawFd) -> Result<bool, io::Error>;
    fn unregister(&mut self, fd: impl AsRawFd) -> Result<bool, io::Error>;

    fn poll(&mut self) -> Result<usize, io::Error>;
}
