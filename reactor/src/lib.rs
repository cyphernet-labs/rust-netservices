#[macro_use]
extern crate amplify;

pub mod schedulers;

mod util;

use std::any::Any;
pub use util::timeout::TimeoutManager;

use std::collections::HashMap;
use std::error::Error as StdError;
use std::fmt::{Debug, Display, Formatter};
use std::hash::Hash;
use std::thread;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crossbeam_channel as chan;

/// Information about generated I/O events from the event loop.
#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Debug)]
pub struct IoSrc<S> {
    /// The source of the I/O event
    pub source: S,
    /// Qualia of the generated event.
    pub io: IoEv,
}

/// Specific I/O events which were received.
#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Debug)]
pub struct IoEv {
    /// Specifies whether I/O source has data to read.
    pub is_readable: bool,
    /// Specifies whether I/O source is ready for write operations.
    pub is_writable: bool,
}

/// Actor is an piece of business logic which depends on I/O and managed in
/// concurrent way by a [`Reactor`] runtime.
///
/// Concrete implementations of the trait should encompass application-specific
/// business logic for working with data. It is advised that they operate as a
/// state machine, advancing on I/O events via calls to [`Actor::io_ready`],
/// dispatched by the reactor runtime.
///
/// Actors should handle such things as handshake, encryption, data encoding,
/// etc and may execute their business logic by calling to other actors or the
/// reactor itself via [`Controller`] handler provided during the actor
/// construction. In such a way they may create new actors and register them
/// with the reactor or send a data to them in a non-blocking way.
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
    /// Each actor must have a unique id within the reactor.
    type Id: Clone + Eq + Hash + Send + Debug + Display;

    /// Extra data provided for constructing the actor from within reactor
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
    /// Advances the state of the resources basing on the results of the I/O.
    ///
    /// The errors returned by this method are forwarded to [`Self::handle_err`].
    fn io_ready(&mut self, io: IoEv) -> Result<(), Self::Error>;

    /// Called by the reactor [`Runtime`] whenever it receives a command for this
    /// resource through the [`Controller`] [`ReactorApi`].
    ///
    /// The errors returned by this method are forwarded to [`Self::handle_err`].
    fn handle_cmd(&mut self, cmd: Self::Cmd) -> Result<(), Self::Error>;

    /// The errors returned by this method are forwarded to [`Broker::handle_err`].
    fn handle_err(&mut self, err: Self::Error) -> Result<(), Self::Error>;
}

/// Implements specific way of scheduling how multiple actors under a
/// [`Reactor`] run in a concurrent way.
pub trait Scheduler<R: Actor>: Iterator<Item = IoSrc<R::Id>> + Send {
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

/// Information for constructing reactor thread pools
pub struct Pool<R: Actor, L: Layout> {
    id: L,
    scheduler: Box<dyn Scheduler<R>>,
    handler: Box<dyn Handler<L>>,
}

impl<R: Actor, L: Layout> Pool<R, L> {
    /// Constructs pool information from the source data
    pub fn new(
        id: L,
        scheduler: impl Scheduler<R> + 'static,
        handler: impl Handler<L> + 'static,
    ) -> Self {
        Pool {
            id,
            scheduler: Box::new(scheduler),
            handler: Box::new(handler),
        }
    }
}

/// Trait layout out the structure for the reactor runtime.
/// It is should be implemented by enumerators listing existing thread pools.
pub trait Layout: Send + Copy + Eq + Hash + Debug + Display + From<u32> + Into<u32> {
    /// The root actor type which will be operated by the reactor
    type RootActor: Actor<Layout = Self>;

    /// Provides reactor with information on constructing thread pools.
    fn default_pools() -> Vec<Pool<Self::RootActor, Self>>;

    /// Convertor transforming any context from the actor hierarchy into the
    /// root actor context.
    fn convert(other_ctx: Box<dyn Any>) -> <Self::RootActor as Actor>::Context;
}

/// Callbacks called in a context of the reactor runtime threads.
pub trait Handler<L: Layout>: Send {
    /// Called on non-actor-specific errors - or on errors which were not held
    /// by the actors
    fn handle_err(&mut self, err: InternalError<L>);
}

/// Implementation of reactor pattern.
///
/// Reactor manages multiple actors of homogenous type `L::RootActor`. Actors
/// can be a TCP connections, file descriptors, GPU kernels or any other
/// process which may be blocked by waiting on I/O events.
///
/// Reactor uses specific [`Scheduler`] implementations (see [`schedulers`]
/// module) which organize operations with multiple actors in a concurrent way.
///
/// Reactor may run multiple independend schedulers of different type.
/// Each scheduler runs in a separate thread. Each actor is allocated to a
/// single scheduler.
///
/// Using multiple schedulers allows to organize different thread pools, like
/// actors operating as workers, network connections etc.
///
/// Reactor waits of the I/O events from any of the actors using
/// [`Scheduler::wait_io`] method and asks corresponding actors to process the
/// events. Timeouts and notifications are also dispatched to a [`Handler`]
/// instance, one per scheduler.
///
/// Reactor, schedulers and actors can be controlled from any actor or any
/// outside thread - or from the handler - by using [`ReactorApi`] and
/// [`Controller`]s constructed by [`Reactor::controller`]. Actors are
/// controlled by sending them commands of [`Actor::Cmd`] type via
/// [`Controller::send`].
pub struct Reactor<L: Layout> {
    /// Threads running schedulers, one per pool.
    scheduler_threads: HashMap<L, JoinHandle<()>>,
    shutdown_send: chan::Sender<()>,
    shutdown_recv: chan::Receiver<()>,
    controller: Controller<L>,
    locked: bool,
}

impl<L: Layout> Reactor<L> {
    /// Constructs reactor and runs it in a thread, returning [`Self`] as a
    /// controller exposing the API ([`ReactorApi`]).
    pub fn new() -> Result<Self, InternalError<L>>
    where
        L: 'static,
    {
        let (shutdown_send, shutdown_recv) = chan::bounded(1);

        let mut reactor = Reactor {
            scheduler_threads: empty!(),
            shutdown_send,
            shutdown_recv: shutdown_recv.clone(),
            controller: Controller::new(),
            locked: false,
        };

        let mut pools = Vec::new();

        // Required to avoid Actor: Send constraint
        struct Info<L: Layout> {
            id: L,
            scheduler: Box<dyn Scheduler<L::RootActor>>,
            control_recv: chan::Receiver<ControlEvent<L::RootActor>>,
            control_send: chan::Sender<ControlEvent<L::RootActor>>,
            shutdown: chan::Receiver<()>,
            handler: Box<dyn Handler<L>>,
        }

        for info in L::default_pools() {
            let (control_send, control_recv) = chan::unbounded();
            let shutdown = shutdown_recv.clone();
            let control = control_send.clone();

            pools.push(Info {
                id: info.id,
                scheduler: info.scheduler,
                control_recv,
                control_send,
                shutdown,
                handler: info.handler,
            });

            reactor.controller.register_pool(info.id, control)?;
        }

        for info in pools {
            let controller = reactor.controller();
            let id = info.id;
            let thread = thread::spawn(move || {
                PoolRuntime::new(
                    info.id,
                    info.scheduler,
                    info.control_recv,
                    info.control_send,
                    info.shutdown,
                    info.handler,
                )
                .run(controller)
            });
            reactor.scheduler_threads.insert(id, thread)
                .ok_or(())
                .expect("controller logic for pool management doesn't account for errors with repeated pool creation");
        }

        Ok(reactor)
    }

    /// Returns controller implementing [`ReactorApi`] for this reactor.
    ///
    /// Once this function is called it wouldn't be possible to add more
    /// pools or actors to the reactor (the reactor state gets locked).
    pub fn controller(&mut self) -> Controller<L> {
        self.locked = true;
        self.controller.clone()
    }

    /// Joins all reactor threads.
    pub fn join(self) -> Result<(), InternalError<L>> {
        for (pool, scheduler_thread) in self.scheduler_threads {
            scheduler_thread
                .join()
                .map_err(|_| InternalError::ThreadError(pool))?;
        }
        Ok(())
    }

    /// Shut downs the reactor.
    pub fn shutdown(self) -> Result<(), InternalError<L>> {
        self.shutdown_send
            .send(())
            .map_err(|_| InternalError::ShutdownChanelBroken)?;
        self.join()?;
        Ok(())
    }
}

/// API for controlling the [`Reactor`] by the reactor instance or through
/// multiple [`Controller`]s constructed by [`Reactor::controller`].
pub trait ReactorApi {
    /// Resource type managed by the reactor.
    type Actor: Actor;

    /// Enumerator for specific reactor runtimes
    type Pool: Layout<RootActor = Self::Actor>;

    /// Connects new resource and adds it to the manager.
    fn start_actor(
        &mut self,
        pool: Self::Pool,
        ctx: <Self::Actor as Actor>::Context,
    ) -> Result<(), InternalError<Self::Pool>>;

    /// Disconnects from a resource, providing a reason.
    fn stop_actor(
        &mut self,
        id: <Self::Actor as Actor>::Id,
    ) -> Result<(), InternalError<Self::Pool>>;

    /// Set one-time timer which will call [`Handler::on_timer`] upon expiration.
    fn set_timer(&mut self, pool: Self::Pool) -> Result<(), InternalError<Self::Pool>>;

    /// Send data to the resource.
    fn send(
        &mut self,
        id: <Self::Actor as Actor>::Id,
        cmd: <Self::Actor as Actor>::Cmd,
    ) -> Result<(), InternalError<Self::Pool>>;
}

/// Instance of reactor controller which may be transferred between threads
pub struct Controller<L: Layout> {
    actor_map: HashMap<<L::RootActor as Actor>::Id, L>,
    channels: HashMap<L, chan::Sender<ControlEvent<L::RootActor>>>,
}

impl<L: Layout> Clone for Controller<L> {
    fn clone(&self) -> Self {
        Controller {
            actor_map: self.actor_map.clone(),
            channels: self.channels.clone(),
        }
    }
}

impl<L: Layout> Controller<L> {
    pub(self) fn new() -> Self {
        Controller {
            actor_map: empty!(),
            channels: empty!(),
        }
    }

    pub(self) fn channel_for(
        &self,
        pool: L,
    ) -> Result<&chan::Sender<ControlEvent<L::RootActor>>, InternalError<L>> {
        self.channels
            .get(&pool)
            .ok_or(InternalError::UnknownPool(pool))
    }

    /// Returns in which pool an actor is run in.
    pub fn pool_for(&self, id: <L::RootActor as Actor>::Id) -> Result<L, InternalError<L>> {
        self.actor_map
            .get(&id)
            .ok_or(InternalError::UnknownActor(id))
            .copied()
    }

    pub(self) fn register_actor(
        &mut self,
        id: <L::RootActor as Actor>::Id,
        pool: L,
    ) -> Result<(), InternalError<L>> {
        if !self.channels.contains_key(&pool) {
            return Err(InternalError::UnknownPool(pool));
        }
        if self.actor_map.contains_key(&id) {
            return Err(InternalError::RepeatedActor(id));
        }
        self.actor_map.insert(id, pool);
        Ok(())
    }

    pub(self) fn register_pool(
        &mut self,
        pool: L,
        channel: chan::Sender<ControlEvent<L::RootActor>>,
    ) -> Result<(), InternalError<L>> {
        if self.channels.contains_key(&pool) {
            return Err(InternalError::RepeatedPoll(pool));
        }
        self.channels.insert(pool, channel);
        Ok(())
    }
}

impl<L: Layout> ReactorApi for Controller<L> {
    type Actor = L::RootActor;
    type Pool = L;

    fn start_actor(
        &mut self,
        pool: L,
        ctx: <Self::Actor as Actor>::Context,
    ) -> Result<(), InternalError<L>> {
        self.channel_for(pool)?.send(ControlEvent::Connect(ctx))?;
        Ok(())
    }

    fn stop_actor(&mut self, id: <Self::Actor as Actor>::Id) -> Result<(), InternalError<L>> {
        let pool = self.pool_for(id.clone())?;
        self.channel_for(pool)?.send(ControlEvent::Disconnect(id))?;
        Ok(())
    }

    fn set_timer(&mut self, pool: L) -> Result<(), InternalError<L>> {
        self.channel_for(pool)?.send(ControlEvent::SetTimer())?;
        Ok(())
    }

    fn send(
        &mut self,
        id: <Self::Actor as Actor>::Id,
        cmd: <Self::Actor as Actor>::Cmd,
    ) -> Result<(), InternalError<L>> {
        let pool = self.pool_for(id.clone())?;
        self.channel_for(pool)?.send(ControlEvent::Send(id, cmd))?;
        Ok(())
    }
}

impl<L: Layout> ReactorApi for Reactor<L> {
    type Actor = L::RootActor;
    type Pool = L;

    fn start_actor(
        &mut self,
        pool: L,
        ctx: <Self::Actor as Actor>::Context,
    ) -> Result<(), InternalError<L>> {
        self.controller.start_actor(pool, ctx)
    }

    fn stop_actor(&mut self, id: <Self::Actor as Actor>::Id) -> Result<(), InternalError<L>> {
        self.controller.stop_actor(id)
    }

    fn set_timer(&mut self, pool: L) -> Result<(), InternalError<L>> {
        self.controller.set_timer(pool)
    }

    fn send(
        &mut self,
        id: <Self::Actor as Actor>::Id,
        cmd: <Self::Actor as Actor>::Cmd,
    ) -> Result<(), InternalError<L>> {
        self.controller.send(id, cmd)
    }
}

/// Runtime represents the reactor event loop with its state handled in a
/// dedicated thread by the reactor. It is controlled by sending instructions
/// through a set of crossbeam channels. [`Reactor`] abstracts that control via
/// exposing high-level [`ReactorApi`] and [`Controller`] objects.
struct PoolRuntime<L: Layout> {
    id: L,
    actors: HashMap<<L::RootActor as Actor>::Id, L::RootActor>,
    scheduler: Box<dyn Scheduler<L::RootActor>>,
    handler: Box<dyn Handler<L>>,
    control_recv: chan::Receiver<ControlEvent<L::RootActor>>,
    control_send: chan::Sender<ControlEvent<L::RootActor>>,
    shutdown: chan::Receiver<()>,
    timeouts: TimeoutManager<()>,
}

impl<L: Layout> PoolRuntime<L> {
    fn new(
        id: L,
        scheduler: Box<dyn Scheduler<L::RootActor>>,
        control_recv: chan::Receiver<ControlEvent<L::RootActor>>,
        control_send: chan::Sender<ControlEvent<L::RootActor>>,
        shutdown: chan::Receiver<()>,
        handler: Box<dyn Handler<L>>,
    ) -> Self {
        PoolRuntime {
            id,
            scheduler,
            actors: empty!(),
            control_recv,
            control_send,
            shutdown,
            handler,
            timeouts: TimeoutManager::new(Duration::from_secs(0)),
        }
    }

    fn run(mut self, controller: Controller<L>) -> ! {
        loop {
            let now = Instant::now();
            if let Err(err) = self.scheduler.wait_io(self.timeouts.next(now)) {
                self.handler
                    .handle_err(InternalError::ActorError(self.id, err));
            }
            for ev in &mut self.scheduler {
                let res = self
                    .actors
                    .get_mut(&ev.source)
                    .expect("resource management inconsistency");
                res.io_ready(ev.io)
                    .or_else(|err| res.handle_err(err))
                    .unwrap_or_else(|err| {
                        self.handler
                            .handle_err(InternalError::ActorError(self.id, err))
                    });
            }
            // TODO: Should we process control events before dispatching input?
            self.process_control(&controller);
            self.process_shutdown();
        }
    }

    fn process_control(&mut self, controller: &Controller<L>) {
        loop {
            match self.control_recv.try_recv() {
                Err(chan::TryRecvError::Disconnected) => {
                    panic!("reactor shutdown channel was dropper")
                }
                Err(chan::TryRecvError::Empty) => break,
                Ok(event) => match event {
                    ControlEvent::Connect(context) => {
                        match L::RootActor::with(context, controller.clone()) {
                            Err(err) => self
                                .handler
                                .handle_err(InternalError::ActorError(self.id, err)),
                            Ok(mut resource) => {
                                self.scheduler
                                    .register_actor(&resource)
                                    .or_else(|err| resource.handle_err(err))
                                    .unwrap_or_else(|err| {
                                        self.handler
                                            .handle_err(InternalError::ActorError(self.id, err))
                                    });
                                self.actors.insert(resource.id(), resource);
                            }
                        };
                        // TODO: Consider to error to the user if the resource was already present
                    }
                    ControlEvent::Disconnect(id) => {
                        self.scheduler.unregister_actor(&id).unwrap_or_else(|err| {
                            self.handler
                                .handle_err(InternalError::ActorError(self.id, err))
                        });
                        self.actors.remove(&id);
                        // TODO: Don't we need to shutdown the resource?
                    }
                    ControlEvent::SetTimer() => {
                        // TODO: Add timeout manager
                    }
                    ControlEvent::Send(id, data) => {
                        if let Some(resource) = self.actors.get_mut(&id) {
                            resource
                                .handle_cmd(data)
                                .or_else(|err| resource.handle_err(err))
                                .unwrap_or_else(|err| {
                                    self.handler
                                        .handle_err(InternalError::ActorError(self.id, err))
                                });
                        }
                    }
                },
            }
        }
    }

    fn process_shutdown(&mut self) {
        match self.shutdown.try_recv() {
            Err(chan::TryRecvError::Empty) => {
                // Nothing to do here
            }
            Ok(()) => {
                // TODO: Disconnect all resources
            }
            Err(chan::TryRecvError::Disconnected) => {
                panic!("reactor shutdown channel was dropper")
            }
        }
    }
}

/// Errors generated by the reactor
#[derive(Display)]
#[display(doc_comments)]
pub enum InternalError<L: Layout> {
    /// shutdown channel in the reactor is broken
    ShutdownChanelBroken,

    /// control channel is broken; unable to send request
    ControlChannelBroken,

    /// actor with id {0} is not known to the reactor
    UnknownActor(<L::RootActor as Actor>::Id),

    /// unknown pool {0}
    UnknownPool(L),

    /// actor with id {0} is already known
    RepeatedActor(<L::RootActor as Actor>::Id),

    /// pool with id {0} is already present in the reactor
    RepeatedPoll(L),

    /// actor on pool {0} has not able to handle error. Details: {1}
    ActorError(L, <L::RootActor as Actor>::Error),

    /// error joining thread pool runtime {0}
    ThreadError(L),
}

// Required due to Derive macro adding R: Debug unnecessary constraint
impl<L: Layout> Debug for InternalError<L> {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            InternalError::ShutdownChanelBroken => f
                .debug_tuple("InternalError::ShutdownChanelBroken")
                .finish(),
            InternalError::ControlChannelBroken => f
                .debug_tuple("InternalError::ControlChannelBroken")
                .finish(),
            InternalError::UnknownActor(id) => f
                .debug_tuple("InternalError::UnknownActor")
                .field(id)
                .finish(),
            InternalError::UnknownPool(pool) => f
                .debug_tuple("InternalError::UnknownPool")
                .field(pool)
                .finish(),
            InternalError::RepeatedActor(id) => f
                .debug_tuple("InternalError::RepeatedActor")
                .field(id)
                .finish(),
            InternalError::RepeatedPoll(pool) => f
                .debug_tuple("InternalError::RepeatedPoll")
                .field(pool)
                .finish(),
            InternalError::ActorError(pool, err) => f
                .debug_tuple("InternalError::ActorError")
                .field(pool)
                .field(err)
                .finish(),
            InternalError::ThreadError(pool) => f
                .debug_tuple("InternalError::ThreadError")
                .field(pool)
                .finish(),
        }
    }
}

impl<L: Layout> StdError for InternalError<L> {}

impl<A: Actor, L: Layout> From<chan::SendError<ControlEvent<A>>> for InternalError<L> {
    fn from(_: chan::SendError<ControlEvent<A>>) -> Self {
        InternalError::ControlChannelBroken
    }
}

/// Events send by [`Controller`] and [`ReactorApi`] to the [`Runtime`].
#[derive(Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Debug)]
enum ControlEvent<A: Actor> {
    /// Request reactor to connect to the resource with some context
    Connect(A::Context),

    /// Request reactor to disconnect from a resource
    Disconnect(A::Id),

    /// Ask reactor to wake up after certain interval
    SetTimer(),

    /// Request reactor to send the data to the resource
    Send(A::Id, A::Cmd),
}
