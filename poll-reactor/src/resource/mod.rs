use std::hash::Hash;
use std::io;
use std::os::unix::io::AsRawFd;

pub enum Event<R: Resource> {
    Connected,
    SessionEstablished,
    Received(R::Message),
    Disconnected(R::DisconnectReason),
}

pub trait Resource: AsRawFd + io::Read + io::Write + Sized + Iterator<Item = Event<Self>> {
    type Id: Copy + Eq + Ord + Hash;
    type Message;
    type DisconnectReason;

    fn id(&self) -> Self::Id;
}
