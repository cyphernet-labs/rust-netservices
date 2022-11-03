#[macro_use]
extern crate amplify;

#[cfg(feature = "popol")]
pub mod popol;
pub mod timeout;

use std::any::Any;
use std::collections::HashMap;
use std::hash::Hash;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use std::{io, thread};

use crossbeam_channel as chan;

use crate::timeout::TimeoutManager;

/// Information about generated I/O events from the event loop.
#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Debug)]
pub struct IoSrc<S> {
    pub source: S,
    pub input: bool,
    pub output: bool,
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
pub trait Resource {
    type Id: Clone + Eq + Ord + Hash + Send;
    type Addr: Send;
    type Error;
    type Data: Send;
    type OutputChannels: Clone + Send;

    fn with(
        addr: Self::Addr,
        controller: Controller<Self>,
        output_channels: Self::OutputChannels,
    ) -> Result<Self, Self::Error>
    where
        Self: Sized;

    fn id(&self) -> Self::Id;

    /// Performs input and/or output operations basing on the flags provided.
    /// For instance, flushes write queue or reads the data and executes
    /// certain business logic on the data read.
    ///
    /// Advances the state of the resources basing on the results of the I/O.
    fn update_from_io(&mut self, input: bool, output: bool) -> Result<(), Self::Error>;

    /// Queues data for sending. The actual sent happens when a [`Self::io`]
    /// with `output` flag set is run.
    fn send(&mut self, data: Self::Data) -> Result<(), Self::Error>;
}

/// Implements specific way of managing multiple resources for a reactor.
/// Blocks on concurrent events from multiple resources.
pub trait IoManager<R: Resource>: Iterator<Item = IoSrc<R::Id>> + Send {
    /// Detects whether a resource under the given id is known to the manager.
    fn has_resource(&self, id: &R::Id) -> bool;

    /// Adds already operating/connected resource to the manager.
    ///
    /// # I/O
    ///
    /// Implementations must not block on the operation or generate any I/O
    /// events.
    fn register_resource(&mut self, resource: &R);

    /// Removes resource from the manager without disconnecting it or generating
    /// any events. Stops resource monitoring and returns the resource itself
    /// (like connection or a TCP stream). May be used later to insert resource
    /// back to the manager with [`Self::register_resource`].
    ///
    /// # I/O
    ///
    /// Implementations must not block on the operation or generate any I/O
    /// events.
    fn unregister_resource(&mut self, id: &R::Id);

    /// Reads events from all resources under this manager.
    ///
    /// # Returns
    ///
    /// Whether the function has timed out.
    ///
    /// # I/O
    ///
    /// Blocks on the read operation or until the timeout.
    fn io_events(&mut self, timeout: Option<Duration>) -> Result<bool, R::Error>;
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
pub struct Reactor<R: Resource> {
    #[allow(dead_code)]
    thread: JoinHandle<()>,
    control: chan::Sender<ControlEvent<R>>,
    shutdown: chan::Sender<()>,
}

impl<R: Resource> Reactor<R> {
    /// Constructs reactor and runs it in a thread, returning [`Self`] as a
    /// controller exposing the API ([`ReactorApi`]).
    pub fn with(
        io: impl IoManager<R> + 'static,
        output_channels: R::OutputChannels,
        err_handler: impl Fn(R::Error) + Send + 'static,
    ) -> io::Result<Self>
    where
        R: 'static,
    {
        let (shutdown_send, shutdown_recv) = chan::bounded(1);
        let (control_send, control_recv) = chan::unbounded();

        let control = control_send.clone();
        let thread = thread::spawn(move || {
            let runtime = Runtime::new(
                io,
                output_channels,
                control_recv,
                control_send,
                shutdown_recv,
                err_handler,
            );
            runtime.run()
        });

        Ok(Reactor {
            thread,
            control,
            shutdown: shutdown_send,
        })
    }

    /// Returns controller implementing [`ReactorApi`] for this reactor.
    pub fn controller(&self) -> Controller<R> {
        Controller {
            control: self.control.clone(),
        }
    }

    /// Joins reactor runtime thread
    pub fn join(self) -> thread::Result<()> {
        self.thread.join()
    }

    /// Shut downs the reactor
    pub fn shutdown(self) -> Result<(), InternalError> {
        self.shutdown
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
    type Resource: Resource;

    /// Connects new resource and adds it to the manager.
    fn connect(&mut self, addr: <Self::Resource as Resource>::Addr) -> Result<(), InternalError>;

    /// Disconnects from a resource, providing a reason.
    fn disconnect(&mut self, id: <Self::Resource as Resource>::Id) -> Result<(), InternalError>;

    /// Set one-time timer which will call [`Handler::on_timer`] upon expiration.
    fn set_timer(&mut self) -> Result<(), InternalError>;

    /// Send data to the resource.
    fn send(
        &mut self,
        id: <Self::Resource as Resource>::Id,
        data: <Self::Resource as Resource>::Data,
    ) -> Result<(), InternalError>;
}

/// Instance of reactor controller which may be transferred between threads
#[derive(Clone)]
pub struct Controller<R: Resource> {
    control: chan::Sender<ControlEvent<R>>,
}

impl<R: Resource> ReactorApi for chan::Sender<ControlEvent<R>> {
    type Resource = R;

    fn connect(&mut self, addr: R::Addr) -> Result<(), InternalError> {
        chan::Sender::send(self, ControlEvent::Connect(addr))?;
        Ok(())
    }

    fn disconnect(&mut self, id: R::Id) -> Result<(), InternalError> {
        chan::Sender::send(self, ControlEvent::Disconnect(id))?;
        Ok(())
    }

    fn set_timer(&mut self) -> Result<(), InternalError> {
        chan::Sender::send(self, ControlEvent::SetTimer())?;
        Ok(())
    }

    fn send(&mut self, id: R::Id, data: R::Data) -> Result<(), InternalError> {
        chan::Sender::send(self, ControlEvent::Send(id, data))?;
        Ok(())
    }
}

impl<R: Resource> ReactorApi for Controller<R> {
    type Resource = R;

    fn connect(&mut self, addr: R::Addr) -> Result<(), InternalError> {
        self.control.connect(addr)
    }

    fn disconnect(&mut self, id: R::Id) -> Result<(), InternalError> {
        self.control.disconnect(id)
    }

    fn set_timer(&mut self) -> Result<(), InternalError> {
        self.control.set_timer()
    }

    fn send(&mut self, id: R::Id, data: R::Data) -> Result<(), InternalError> {
        ReactorApi::send(&mut self.control, id, data)
    }
}

impl<R: Resource> ReactorApi for Reactor<R> {
    type Resource = R;

    fn connect(&mut self, addr: R::Addr) -> Result<(), InternalError> {
        self.control.connect(addr)
    }

    fn disconnect(&mut self, id: R::Id) -> Result<(), InternalError> {
        self.control.disconnect(id)
    }

    fn set_timer(&mut self) -> Result<(), InternalError> {
        self.control.set_timer()
    }

    fn send(&mut self, id: R::Id, data: R::Data) -> Result<(), InternalError> {
        ReactorApi::send(&mut self.control, id, data)
    }
}

/// Runtime represents the reactor event loop with its state handled in a
/// dedicated thread by the reactor. It is controlled by sending instructions
/// through a set of crossbeam channels. [`Reactor`] abstracts that control via
/// exposing high-level [`ReactorApi`] and [`Controller`] objects.
struct Runtime<R: Resource, IO: IoManager<R>, EH: Fn(R::Error)> {
    resources: HashMap<R::Id, R>,
    io: IO,
    err_handler: EH,
    output_channels: R::OutputChannels,
    control_recv: chan::Receiver<ControlEvent<R>>,
    control_send: chan::Sender<ControlEvent<R>>,
    shutdown: chan::Receiver<()>,
    timeouts: TimeoutManager<()>,
}

impl<R: Resource, IO: IoManager<R>, EH: Fn(R::Error)> Runtime<R, IO, EH> {
    fn new(
        io: IO,
        output_channels: R::OutputChannels,
        control_recv: chan::Receiver<ControlEvent<R>>,
        control_send: chan::Sender<ControlEvent<R>>,
        shutdown: chan::Receiver<()>,
        err_handler: EH,
    ) -> Self {
        Runtime {
            io,
            output_channels,
            resources: empty!(),
            control_recv,
            control_send,
            shutdown,
            err_handler,
            timeouts: TimeoutManager::new(Duration::from_secs(0)),
        }
    }

    fn run(mut self) -> ! {
        loop {
            let now = Instant::now();
            if let Err(err) = self.io.io_events(self.timeouts.next(now)) {
                // We ignore events here since it is up to the user to drop the
                // error channel and ignore them
                let _ = (self.err_handler)(err);
            }
            // TODO: Should we process control events before dispatching input?
            self.process_control();
            self.process_shutdown();
        }
    }

    fn process_control(&mut self) {
        loop {
            match self.control_recv.try_recv() {
                Err(chan::TryRecvError::Disconnected) => {
                    panic!("reactor shutdown channel was dropper")
                }
                Err(chan::TryRecvError::Empty) => break,
                Ok(event) => match event {
                    ControlEvent::Connect(addr) => {
                        let controller = Controller {
                            control: self.control_send.clone(),
                        };
                        match R::with(addr, controller, self.output_channels.clone()) {
                            Err(err) => (self.err_handler)(err),
                            Ok(resource) => {
                                self.io.register_resource(&resource);
                                self.resources.insert(resource.id(), resource);
                            }
                        };
                        // TODO: Consider to error to the user if the resource was already present
                    }
                    ControlEvent::Disconnect(id) => {
                        self.io.unregister_resource(&id);
                        self.resources.remove(&id);
                        // TODO: Don't we need to shutdown the resource?
                        // TODO: Consider to error to the user if the resource is not found
                    }
                    ControlEvent::SetTimer() => {
                        // TODO: Add timeout manager
                    }
                    ControlEvent::Send(id, data) => {
                        if let Some(resource) = self.resources.get_mut(&id) {
                            let _ = resource.send(data);
                        }
                        // TODO: Consider to error to the user if the resource is not found
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

#[derive(Debug, Display, Error, From)]
#[display(doc_comments)]
pub enum InternalError {
    /// shutdown channel in the reactor is broken
    ShutdownChanelBroken,

    /// control channel is broken; unable to send request
    ControlChannelBroken,

    /// error joining runtime
    #[from]
    ThreadError(Box<dyn Any + Send + 'static>),
}

impl<R: Resource> From<chan::SendError<ControlEvent<R>>> for InternalError {
    fn from(_: chan::SendError<ControlEvent<R>>) -> Self {
        InternalError::ControlChannelBroken
    }
}

/// Events send by [`Controller`] and [`ReactorApi`] to the [`Runtime`].
#[derive(Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Debug)]
enum ControlEvent<R: Resource> {
    /// Request reactor to connect to the resource by address
    Connect(R::Addr),

    /// Request reactor to disconnect from a resource
    Disconnect(R::Id),

    /// Ask reactor to wake up after certain interval
    SetTimer(),

    /// Request reactor to send the data to the resource
    Send(R::Id, R::Data),
}
