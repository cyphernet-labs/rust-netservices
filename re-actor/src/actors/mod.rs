//! Actor is an piece of business logic which depends on I/O and managed in
//! concurrent way by a [`Reactor`] runtime.
//!
//! Actors handle such things as handshake, encryption, data encoding,
//! etc and may execute their business logic by calling to other actors or the
//! re-actor itself via [`Controller`] handler provided during the actor
//! construction. In such a way they may create new actors and register them
//! with the re-actor or send a data to them in a non-blocking way.
//! If an actor needs to perform extensive or blocking operation it is advised
//! to use dedicated worker threads in a separate pools under [`Reactor`].
//!
//! Actors can be composed from other actors, which helps with abstraction and
//! concern separation. For instance, an actor providing TCP connection may be
//! composed into an actor which performs encoding on that stream - and then
//! into actor providing some framing protocol etc.

#[cfg(feature = "mio")]
pub mod mio;
#[cfg(feature = "connect_nonblocking")]
pub mod socket2;
pub mod stdfd;
pub mod stdtcp;
#[cfg(feature = "zmq")]
pub mod zeromq;

use std::error::Error as StdError;
use std::fmt::{Debug, Display};
use std::hash::Hash;

use crate::{Controller, Layout};

/// Actor is an piece of business logic which depends on I/O and managed in
/// concurrent way by a [`Reactor`] runtime.
///
/// Concrete implementations of the trait should encompass application-specific
/// business logic for working with data. It is advised that they operate as a
/// state machine, advancing on I/O events via calls to [`Actor::io_ready`],
/// dispatched by the re-actor runtime.
///
/// Actors should handle such things as handshake, encryption, data encoding,
/// etc and may execute their business logic by calling to other actors or the
/// re-actor itself via [`Controller`] handler provided during the actor
/// construction. In such a way they may create new actors and register them
/// with the re-actor or send a data to them in a non-blocking way.
/// If an actor needs to perform extensive or blocking operation it is advised
/// to use dedicated worker threads in a separate pools under [`Reactor`].
///
/// Actors can be composed from other actors, which helps with abstraction and
/// concern separation. For instance, an actor providing TCP connection may be
/// composed into an actor which performs encoding on that stream - and then
/// into actor providing some framing protocol etc.
pub trait Actor {
    /// Thread pools layout this actor operates under. For actors which can
    /// operate under multiple pools this associated type must be converted
    /// into generic parameter.
    type Layout: Layout;

    /// Actor's id types.
    ///
    /// Each actor must have a unique id within the re-actor.
    type Id: Clone + Eq + Hash + Send + Debug + Display;

    /// Extra data provided for constructing the actor from within re-actor
    /// runtime (see [`Controller::start_actor`].
    type Context: Send;

    /// Set of commands supported by the actor's business logic.
    ///
    /// In case of a single command this can be just a data type providing
    /// parameters to that command (for instance a byte string for an
    /// actor operating as a writer).
    type Cmd: Send;

    /// Actor-specific error type, returned from I/O events handling or
    /// command-processing business logic.
    type Error: StdError;

    /// Constructs actor giving some `context`. Each actor is provided with the
    /// controller, which it should store internally in cases when it requires
    /// operating with other actors.
    fn with(
        context: Self::Context,
        controller: Controller<Self::Layout>,
    ) -> Result<Self, Self::Error>
    where
        Self: Sized;

    /// Returns actor's id.
    fn id(&self) -> Self::Id;

    /// Performs input and/or output operations basing on the flags provided.
    /// For instance, flushes write queue or reads the data and executes
    /// certain business logic on the data read.
    ///
    /// Advances the state of the reactor basing on the results of the I/O.
    ///
    /// The errors returned by this method are forwarded to [`Self::handle_err`].
    fn io_ready(&mut self, io: IoEv) -> Result<(), Self::Error>;

    /// Called by the re-actor [`Runtime`] whenever it receives a command for this
    /// resource through the [`Controller`] [`ReactorApi`].
    ///
    /// The errors returned by this method are forwarded to [`Self::handle_err`].
    fn handle_cmd(&mut self, cmd: Self::Cmd) -> Result<(), Self::Error>;

    /// The errors returned by this method are forwarded to [`Broker::handle_err`].
    fn handle_err(&mut self, err: Self::Error) -> Result<(), Self::Error>;
}

/// Information about generated I/O events from the event loop.
#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Debug)]
pub struct IoSrc<S> {
    /// The source of the I/O event
    pub source: S,
    /// Qualia of the generated event.
    pub io: IoEv,
}

/// Information about I/O events which has happened for an actor
#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Debug)]
pub struct IoEv {
    /// Specifies whether I/O source has data to read.
    pub is_readable: bool,
    /// Specifies whether I/O source is ready for write operations.
    pub is_writable: bool,
}
