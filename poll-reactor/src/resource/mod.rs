use std::hash::Hash;
use std::io;
use std::os::unix::io::AsRawFd;

use crate::poller::IoEv;

pub trait Resource: AsRawFd + io::Write + Iterator<Item = Self::Event> {
    type Id: Copy + Eq + Ord + Hash;
    type Event;
    type Error: std::error::Error + Send + 'static;

    fn id(&self) -> Self::Id;

    /// Asks resource to handle I/O. Must return a number of events generated
    /// from the resource I/O.
    fn handle_io(&mut self, ev: IoEv) -> Result<usize, Self::Error>;

    fn disconnect(&mut self) -> Result<(), Self::Error>;
}
