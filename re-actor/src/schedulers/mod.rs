mod crossbeam;
#[cfg(feature = "epoll")]
mod epoll;
#[cfg(feature = "mio")]
mod mio;
#[cfg(feature = "polling")]
mod polling;
#[cfg(feature = "popol")]
mod popol;
mod threaded;
#[cfg(feature = "zmq")]
mod zeromq;

pub use self::crossbeam::CrossbeamScheduler;
#[cfg(feature = "polling")]
pub use self::polling::PollingScheduler;
#[cfg(feature = "popol")]
pub use self::popol::PopolScheduler;

use std::time::Duration;

use crate::actors::{Actor, IoSrc};

/// Implements specific way of scheduling how multiple actors under a
/// [`Reactor`] run in a concurrent way.
pub trait Scheduler<R: Actor>: Iterator<Item = IoSrc<R::IoResource>> + Send {
    /// Detects whether a resource under the given id is known to the manager.
    fn has_actor(&self, id: &R::Id) -> bool;

    /// Adds already operating/connected actor to the scheduler.
    ///
    /// # I/O
    ///
    /// Implementations must not block on the operation or generate any I/O
    /// events.
    fn register_actor(&mut self, actor: &R) -> Result<(), R::Error>;

    /// Removes previously added actor from the scheduler without generating
    /// any events. Stops actor run scheduling.
    ///
    /// # I/O
    ///
    /// Implementations must not block on the operation or generate any I/O
    /// events.
    fn unregister_actor(&mut self, id: &R::Id) -> Result<(), R::Error>;

    /// Waits for I/O events from all actors under this scheduler.
    ///
    /// # Returns
    ///
    /// Whether the function has timed out.
    ///
    /// # I/O
    ///
    /// Blocks until the timeout.
    fn wait_io(&mut self, timeout: Option<Duration>) -> Result<bool, R::Error>;
}
