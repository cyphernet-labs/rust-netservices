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

/// Resource is an I/O item operated by the [`crate::Reactor`]. It should
/// encompass all application-specific business logic for working with I/O
/// data and can operate as a state machine, advancing on I/O events via
/// calls to [`Resource::update_from_io`], dispatched by the reactor runtime.
/// Resources should handle such things as handshake, encryption, data encoding,
/// etc. and may execute their business logic by calling the reactor via
/// [`Controller`] handler provided during the resource construction. In such a
/// way they may create new resources and register them with the reactor,
/// disconnect other resources or send a data to them in a non-blocking way.
/// If a resource needs to perform extensive or blocking operation it is advised
/// to use dedicated worker threads. While this can be handled by the resource
/// internally, if the worker thread pool is desired it can be talked to via
/// set of channels specified as [`Resource::OutputChannels`] associated type
/// and provided to the resource upon its construction.
pub trait Actor {
    /// Actor's id types.
    ///
    /// Each actor must have a unique id without a reactor.
    type Id: Clone + Eq + Ord + Hash + Send + Debug + Display;

    /// Extra data for actor construction
    type Context: Send;

    /// Set of commands supported by the actor's business logic.
    ///
    /// In case of a single command this can be just a data type providing
    /// parameters to that command (for instance a byte string for an
    /// actor operting as a writer).
    type Cmd: Send;

    /// Actor-specific error type, returned from I/O events handling or
    /// command-processing business logic.
    type Error: StdError;

    /// Thread pool system this actor operates under. For actors which can
    /// operate under multiple pools this associated type must be converted
    /// into generic parameter.
    type PoolSystem: Pool;

    /// Constructs actor giving some `context`. Each actor is provided with the
    /// controller, which it should store internally in cases when it requires
    /// operating with other actors.
    fn with(
        context: Self::Context,
        controller: Controller<Self::PoolSystem>,
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

/// Implements specific way of managing multiple resources for a reactor.
/// Blocks on concurrent events from multiple resources.
pub trait Scheduler<R: Actor>: Iterator<Item = IoSrc<R::Id>> + Send {
    /// Detects whether a resource under the given id is known to the manager.
    fn has_actor(&self, id: &R::Id) -> bool;

    /// Adds already operating/connected resource to the manager.
    ///
    /// # I/O
    ///
    /// Implementations must not block on the operation or generate any I/O
    /// events.
    fn register_actor(&mut self, resource: &R) -> Result<(), R::Error>;

    /// Removes resource from the manager without disconnecting it or generating
    /// any events. Stops resource monitoring and returns the resource itself
    /// (like connection or a TCP stream). May be used later to insert resource
    /// back to the manager with [`Self::register_resource`].
    ///
    /// # I/O
    ///
    /// Implementations must not block on the operation or generate any I/O
    /// events.
    fn unregister_actor(&mut self, id: &R::Id) -> Result<(), R::Error>;

    /// Reads events from all resources under this manager.
    ///
    /// # Returns
    ///
    /// Whether the function has timed out.
    ///
    /// # I/O
    ///
    /// Blocks on the read operation or until the timeout.
    fn wait_io(&mut self, timeout: Option<Duration>) -> Result<bool, R::Error>;
}

/// Callbacks called in a context of the reactor runtime threads.
pub trait Handler<P: Pool>: Send {
    /// Called on non-actor-specific errors - or on errors which were not held by the resources
    fn handle_err(&mut self, err: InternalError<P>);
}

/// Information for constructing reactor thread pools
pub struct PoolInfo<R: Actor, P: Pool> {
    id: P,
    scheduler: Box<dyn Scheduler<R>>,
    handler: Box<dyn Handler<P>>,
}

impl<R: Actor, P: Pool> PoolInfo<R, P> {
    pub fn new(
        id: P,
        scheduler: impl Scheduler<R> + 'static,
        handler: impl Handler<P> + 'static,
    ) -> Self {
        PoolInfo {
            id,
            scheduler: Box::new(scheduler),
            handler: Box::new(handler),
        }
    }
}

/// Trait for an enumeration of pools for a reactor
pub trait Pool: Send + Copy + Eq + Hash + Debug + Display + From<u32> + Into<u32> {
    type RootActor: Actor<PoolSystem = Self>;
    fn default_pools() -> Vec<PoolInfo<Self::RootActor, Self>>;
    fn convert(other_ctx: Box<dyn Any>) -> <Self::RootActor as Actor>::Context;
}

/// Implementation of reactor pattern.
///
/// Reactor manages multiple resources of homogenous type `R` (resource can be a
/// TCP connections, file descriptors or any other blocking resource). It does
/// concurrent read of the I/O events from the resources using
/// [`ResourceMgr::read_events`] method, and dispatches events to a
/// [`Handler`] in synchronous demultiplexed way. Finally, it can be controlled
/// from any outside thread - or from the handler - by using [`ReactorApi`] and
/// [`Controllers`] constructed by [`Reactor::controller`]. This includes ability
/// to connect or disconnect resources or send them data.
///
/// Reactor manages internally a thread which runs the [`Runtime`] event loop.
pub struct Reactor<P: Pool> {
    /// Threads running schedulers, one per pool.
    scheduler_threads: HashMap<P, JoinHandle<()>>,
    shutdown_send: chan::Sender<()>,
    shutdown_recv: chan::Receiver<()>,
    controller: Controller<P>,
    locked: bool,
}

impl<P: Pool> Reactor<P> {
    /// Constructs reactor and runs it in a thread, returning [`Self`] as a
    /// controller exposing the API ([`ReactorApi`]).
    pub fn new() -> Result<Self, InternalError<P>>
    where
        P: 'static,
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
        struct Info<P: Pool> {
            id: P,
            scheduler: Box<dyn Scheduler<P::RootActor>>,
            control_recv: chan::Receiver<ControlEvent<P::RootActor>>,
            control_send: chan::Sender<ControlEvent<P::RootActor>>,
            shutdown: chan::Receiver<()>,
            handler: Box<dyn Handler<P>>,
        }

        for info in P::default_pools() {
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
    pub fn controller(&mut self) -> Controller<P> {
        self.locked = true;
        self.controller.clone()
    }

    /// Joins all reactor threads.
    pub fn join(self) -> Result<(), InternalError<P>> {
        for (pool, scheduler_thread) in self.scheduler_threads {
            scheduler_thread
                .join()
                .map_err(|_| InternalError::ThreadError(pool))?;
        }
        Ok(())
    }

    /// Shut downs the reactor.
    pub fn shutdown(self) -> Result<(), InternalError<P>> {
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
    type Pool: Pool<RootActor = Self::Actor>;

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
pub struct Controller<P: Pool> {
    actor_map: HashMap<<P::RootActor as Actor>::Id, P>,
    channels: HashMap<P, chan::Sender<ControlEvent<P::RootActor>>>,
}

impl<P: Pool> Clone for Controller<P> {
    fn clone(&self) -> Self {
        Controller {
            actor_map: self.actor_map.clone(),
            channels: self.channels.clone(),
        }
    }
}

impl<P: Pool> Controller<P> {
    pub(self) fn new() -> Self {
        Controller {
            actor_map: empty!(),
            channels: empty!(),
        }
    }

    pub(self) fn channel_for(
        &self,
        pool: P,
    ) -> Result<&chan::Sender<ControlEvent<P::RootActor>>, InternalError<P>> {
        self.channels
            .get(&pool)
            .ok_or(InternalError::UnknownPool(pool))
    }

    /// Returns in which pool an actor is run in.
    pub fn pool_for(&self, id: <P::RootActor as Actor>::Id) -> Result<P, InternalError<P>> {
        self.actor_map
            .get(&id)
            .ok_or(InternalError::UnknownActor(id))
            .copied()
    }

    pub(self) fn register_actor(
        &mut self,
        id: <P::RootActor as Actor>::Id,
        pool: P,
    ) -> Result<(), InternalError<P>> {
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
        pool: P,
        channel: chan::Sender<ControlEvent<P::RootActor>>,
    ) -> Result<(), InternalError<P>> {
        if self.channels.contains_key(&pool) {
            return Err(InternalError::RepeatedPoll(pool));
        }
        self.channels.insert(pool, channel);
        Ok(())
    }
}

impl<P: Pool> ReactorApi for Controller<P> {
    type Actor = P::RootActor;
    type Pool = P;

    fn start_actor(
        &mut self,
        pool: P,
        ctx: <Self::Actor as Actor>::Context,
    ) -> Result<(), InternalError<P>> {
        self.channel_for(pool)?.send(ControlEvent::Connect(ctx))?;
        Ok(())
    }

    fn stop_actor(&mut self, id: <Self::Actor as Actor>::Id) -> Result<(), InternalError<P>> {
        let pool = self.pool_for(id.clone())?;
        self.channel_for(pool)?.send(ControlEvent::Disconnect(id))?;
        Ok(())
    }

    fn set_timer(&mut self, pool: P) -> Result<(), InternalError<P>> {
        self.channel_for(pool)?.send(ControlEvent::SetTimer())?;
        Ok(())
    }

    fn send(
        &mut self,
        id: <Self::Actor as Actor>::Id,
        cmd: <Self::Actor as Actor>::Cmd,
    ) -> Result<(), InternalError<P>> {
        let pool = self.pool_for(id.clone())?;
        self.channel_for(pool)?.send(ControlEvent::Send(id, cmd))?;
        Ok(())
    }
}

impl<P: Pool> ReactorApi for Reactor<P> {
    type Actor = P::RootActor;
    type Pool = P;

    fn start_actor(
        &mut self,
        pool: P,
        ctx: <Self::Actor as Actor>::Context,
    ) -> Result<(), InternalError<P>> {
        self.controller.start_actor(pool, ctx)
    }

    fn stop_actor(&mut self, id: <Self::Actor as Actor>::Id) -> Result<(), InternalError<P>> {
        self.controller.stop_actor(id)
    }

    fn set_timer(&mut self, pool: P) -> Result<(), InternalError<P>> {
        self.controller.set_timer(pool)
    }

    fn send(
        &mut self,
        id: <Self::Actor as Actor>::Id,
        cmd: <Self::Actor as Actor>::Cmd,
    ) -> Result<(), InternalError<P>> {
        self.controller.send(id, cmd)
    }
}

/// Runtime represents the reactor event loop with its state handled in a
/// dedicated thread by the reactor. It is controlled by sending instructions
/// through a set of crossbeam channels. [`Reactor`] abstracts that control via
/// exposing high-level [`ReactorApi`] and [`Controller`] objects.
struct PoolRuntime<P: Pool> {
    id: P,
    actors: HashMap<<P::RootActor as Actor>::Id, P::RootActor>,
    scheduler: Box<dyn Scheduler<P::RootActor>>,
    handler: Box<dyn Handler<P>>,
    control_recv: chan::Receiver<ControlEvent<P::RootActor>>,
    control_send: chan::Sender<ControlEvent<P::RootActor>>,
    shutdown: chan::Receiver<()>,
    timeouts: TimeoutManager<()>,
}

impl<P: Pool> PoolRuntime<P> {
    fn new(
        id: P,
        scheduler: Box<dyn Scheduler<P::RootActor>>,
        control_recv: chan::Receiver<ControlEvent<P::RootActor>>,
        control_send: chan::Sender<ControlEvent<P::RootActor>>,
        shutdown: chan::Receiver<()>,
        handler: Box<dyn Handler<P>>,
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

    fn run(mut self, controller: Controller<P>) -> ! {
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

    fn process_control(&mut self, controller: &Controller<P>) {
        loop {
            match self.control_recv.try_recv() {
                Err(chan::TryRecvError::Disconnected) => {
                    panic!("reactor shutdown channel was dropper")
                }
                Err(chan::TryRecvError::Empty) => break,
                Ok(event) => match event {
                    ControlEvent::Connect(context) => {
                        match P::RootActor::with(context, controller.clone()) {
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

#[derive(Display)]
#[display(doc_comments)]
pub enum InternalError<P: Pool> {
    /// shutdown channel in the reactor is broken
    ShutdownChanelBroken,

    /// control channel is broken; unable to send request
    ControlChannelBroken,

    /// actor with id {0} is not known to the reactor
    UnknownActor(<P::RootActor as Actor>::Id),

    /// unknown pool {0}
    UnknownPool(P),

    /// actor with id {0} is already known
    RepeatedActor(<P::RootActor as Actor>::Id),

    /// pool with id {0} is already present in the reactor
    RepeatedPoll(P),

    /// actor on pool {0} has not able to handle error. Details: {1}
    ActorError(P, <P::RootActor as Actor>::Error),

    /// error joining thread pool runtime {0}
    ThreadError(P),
}

// Required due to Derive macro adding R: Debug unnecessary constraint
impl<P: Pool> Debug for InternalError<P> {
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

impl<P: Pool> StdError for InternalError<P> {}

impl<R: Actor, P: Pool> From<chan::SendError<ControlEvent<R>>> for InternalError<P> {
    fn from(_: chan::SendError<ControlEvent<R>>) -> Self {
        InternalError::ControlChannelBroken
    }
}

/// Events send by [`Controller`] and [`ReactorApi`] to the [`Runtime`].
#[derive(Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Debug)]
enum ControlEvent<R: Actor> {
    /// Request reactor to connect to the resource with some context
    Connect(R::Context),

    /// Request reactor to disconnect from a resource
    Disconnect(R::Id),

    /// Ask reactor to wake up after certain interval
    SetTimer(),

    /// Request reactor to send the data to the resource
    Send(R::Id, R::Cmd),
}
