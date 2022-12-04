pub mod socket;

use std::fmt::{Debug, Display};
use std::hash::Hash;
use std::os::unix::io::AsRawFd;
use std::time::Duration;
use std::{io, net};

use crate::poller::IoEv;

/// Maximum time to wait when reading from a socket.
const READ_TIMEOUT: Duration = Duration::from_secs(6);
/// Maximum time to wait when writing to a socket.
const WRITE_TIMEOUT: Duration = Duration::from_secs(3);

pub trait ResourceId: Copy + Eq + Ord + Hash + Debug + Display {}

pub trait Resource: AsRawFd + Iterator<Item = Self::Event> {
    type Id: ResourceId;
    type Event;
    type Message;

    fn id(&self) -> Self::Id;

    /// Asks resource to handle I/O. Must return a number of events generated
    /// from the resource I/O.
    fn handle_io(&mut self, ev: IoEv) -> usize;

    fn send(&mut self, msg: Self::Message) -> Result<(), io::Error>;

    fn disconnect(self);
}

impl ResourceId for net::SocketAddr {}
