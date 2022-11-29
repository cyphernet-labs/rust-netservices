use std::fmt::{Debug, Display};
use std::hash::Hash;
use std::io;
use std::os::unix::io::AsRawFd;

use crate::poller::IoEv;

pub trait ResourceId: Copy + Eq + Ord + Hash + Debug + Display {}

pub trait Resource: AsRawFd + io::Write + Iterator<Item = Self::Event> {
    type Id: ResourceId;
    type Event;
    type Error: std::error::Error + Send + 'static;

    fn id(&self) -> Self::Id;

    /// Asks resource to handle I/O. Must return a number of events generated
    /// from the resource I/O.
    fn handle_io(&mut self, ev: IoEv) -> usize;

    fn disconnect(&mut self);
}
