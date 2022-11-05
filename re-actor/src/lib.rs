//! Implementation of the re-actor pattern.
//!
//! Reactor manages multiple [`Actor`]s, which represent a modules of application
//! business logic which depend on I/O events. Actors
//! can be a TCP connections, file descriptors, GPU kernels or any other
//! process which may be blocked by waiting on I/O events.
//!
//! Reactor uses specific [`Scheduler`] implementations (see [`schedulers`]
//! module) which organize operations with multiple actors in a concurrent way.
//!
//! Reactor may run multiple independent schedulers of different type.
//! Each scheduler runs in a separate thread. Each actor is allocated to a
//! single scheduler.
//!
//! Using multiple schedulers allows to organize different thread pools, like
//! actors operating as workers, network connections etc.
//!
//! Reactor waits of the I/O events from any of the actors using
//! [`Scheduler::wait_io`] method and asks corresponding actors to process the
//! events. Timeouts and notifications are also dispatched to a [`Handler`]
//! instance, one per scheduler.
//!
//! Reactor, schedulers and actors can be controlled from any actor or any
//! outside thread - or from the handler - by using [`ReactorApi`] and
//! [`Controller`]s constructed by [`Reactor::controller`]. Actors are
//! controlled by sending them commands of [`Actor::Cmd`] type via
//! [`Controller::send`].

#[macro_use]
extern crate amplify;

pub mod actors;
mod reactor;
pub mod schedulers;
mod util;

pub use actors::Actor;
pub use reactor::{Controller, Handler, InternalError, Layout, Pool, Reactor, ReactorApi};
pub use schedulers::Scheduler;
pub use util::timeout::TimeoutManager;
