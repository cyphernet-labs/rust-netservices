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

#[cfg(feature = "polling")]
pub use self::polling::PollingScheduler;
#[cfg(feature = "popol")]
pub use self::popol::PopolScheduler;
