//! Actors based on crossbeam channels, not backed by any other I/O.
//! Useful in combination with [`crate::CrossbeamScheduler`].

use crossbeam_channel as chan;

use crate::Actor;

pub trait CrossbeamActor<T>
where
    Self: Actor<IoResource = chan::Receiver<T>>,
    Self::Cmd: From<T>,
{
}
