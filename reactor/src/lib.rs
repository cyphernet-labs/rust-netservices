#[macro_use]
extern crate amplify;

pub mod managers;
pub mod resources;

use std::thread::JoinHandle;
use std::{io, thread};

use crossbeam_channel as chan;
use streampipes::Resource;

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
    pub fn with<H: Handler<R>>(
        resources: impl ResourceMgr<R> + 'static,
        handler_context: H::Context,
    ) -> io::Result<Self>
    where
        R: 'static,
        H::Context: 'static,
    {
        let (shutdown_send, shutdown_recv) = chan::bounded(1);
        let (control_send, control_recv) = chan::unbounded();

        let controller = Controller {
            control: control_send.clone(),
        };
        let thread = thread::spawn(move || {
            let handler = H::new(controller, handler_context);
            let mut dispatcher = Runtime::new(resources, handler, control_recv, shutdown_recv);
            dispatcher.run()
        });

        Ok(Reactor {
            thread,
            control: control_send,
            shutdown: shutdown_send,
        })
    }

    /// Returns controller implementing [`ReactorApi`] for this reactor.
    pub fn controller(&self) -> Controller<R> {
        Controller {
            control: self.control.clone(),
        }
    }

    /// To shutdown the reactor just drop it.
    fn shutdown(&mut self) -> Result<(), chan::SendError<()>> {
        self.shutdown.send(())
    }
}

/// To shutdown the reactor just drop it.
impl<R: Resource> Drop for Reactor<R> {
    fn drop(&mut self) {
        self.shutdown().expect("unable to shutdown the reactor");
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
    fn disconnect(&mut self, addr: <Self::Resource as Resource>::Addr)
        -> Result<(), InternalError>;

    /// Set one-time timer which will call [`Handler::on_timer`] upon expiration.
    fn set_timer(&mut self) -> Result<(), InternalError>;

    /// Send data to the resource.
    fn send(
        &mut self,
        addr: <Self::Resource as Resource>::Addr,
        data: Vec<u8>,
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

    fn disconnect(&mut self, addr: R::Addr) -> Result<(), InternalError> {
        chan::Sender::send(self, ControlEvent::Disconnect(addr))?;
        Ok(())
    }

    fn set_timer(&mut self) -> Result<(), InternalError> {
        chan::Sender::send(self, ControlEvent::SetTimer())?;
        Ok(())
    }

    fn send(&mut self, addr: R::Addr, data: Vec<u8>) -> Result<(), InternalError> {
        chan::Sender::send(self, ControlEvent::Send(addr, data))?;
        Ok(())
    }
}

impl<R: Resource> ReactorApi for Controller<R> {
    type Resource = R;

    fn connect(&mut self, addr: R::Addr) -> Result<(), InternalError> {
        self.control.connect(addr)
    }

    fn disconnect(&mut self, addr: R::Addr) -> Result<(), InternalError> {
        self.control.disconnect(addr)
    }

    fn set_timer(&mut self) -> Result<(), InternalError> {
        self.control.set_timer()
    }

    fn send(&mut self, addr: R::Addr, data: Vec<u8>) -> Result<(), InternalError> {
        ReactorApi::send(&mut self.control, addr, data)
    }
}

impl<R: Resource> ReactorApi for Reactor<R> {
    type Resource = R;

    fn connect(&mut self, addr: R::Addr) -> Result<(), InternalError> {
        self.control.connect(addr)
    }

    fn disconnect(&mut self, addr: R::Addr) -> Result<(), InternalError> {
        self.control.disconnect(addr)
    }

    fn set_timer(&mut self) -> Result<(), InternalError> {
        self.control.set_timer()
    }

    fn send(&mut self, addr: R::Addr, data: Vec<u8>) -> Result<(), InternalError> {
        ReactorApi::send(&mut self.control, addr, data)
    }
}

/// Runtime represents the reactor event loop with its state handled in a
/// dedicated thread by the reactor. It is controlled by sending instructions
/// through a set of crossbeam channels. [`Reactor`] abstracts that control via
/// exposing high-level [`ReactorApi`] and [`Controller`] objects.
struct Runtime<R: Resource, M: ResourceMgr<R>, H: Handler<R>> {
    resources: M,
    handler: H,
    control: chan::Receiver<ControlEvent<R>>,
    shutdown: chan::Receiver<()>,
}

impl<R: Resource, M: ResourceMgr<R>, H: Handler<R>> Runtime<R, M, H> {
    fn new(
        resources: M,
        handler: H,
        control: chan::Receiver<ControlEvent<R>>,
        shutdown: chan::Receiver<()>,
    ) -> Self {
        Runtime {
            resources,
            handler,
            control,
            shutdown,
        }
    }

    fn run(&mut self) -> ! {
        loop {
            self.dispatch()
                .unwrap_or_else(|err| self.handler.resource_error(err));
            // Should we process control events before dispatching input?
            self.process_control()
                .unwrap_or_else(|err| self.handler.resource_error(err));
            self.process_shutdown()
                .unwrap_or_else(|err| self.handler.internal_failure(err));
        }
    }

    fn dispatch(&mut self) -> Result<(), R::Error> {
        let queue = self.resources.read_events()?;
        self.handler.tick();

        for event in queue {
            match event {
                InputEvent::Spawn(_) => { /* Nothing to do here */ }
                InputEvent::Upgraded() => { /* Nothing to do here */ }
                InputEvent::Connected {
                    remote_addr,
                    direction,
                } => self.handler.connected(remote_addr, direction),
                InputEvent::Disconnected(addr, reason) => self.handler.disconnected(addr, reason),
                InputEvent::Timer => self.handler.on_timer(),
                InputEvent::Timeout => { /* Nothing to do here */ }
                InputEvent::Received(addr, msg) => self.handler.received(addr, msg),
            }
        }

        Ok(())
    }

    fn process_control(&mut self) -> Result<(), R::Error> {
        loop {
            match self.control.try_recv() {
                Err(chan::TryRecvError::Disconnected) => {
                    self.handler
                        .internal_failure(InternalError::ShutdownChanelBroken);
                    break;
                }
                Err(chan::TryRecvError::Empty) => break,
                Ok(event) => match event {
                    ControlEvent::Connect(_) => {}
                    ControlEvent::Disconnect(_) => {}
                    ControlEvent::SetTimer() => {}
                    ControlEvent::Send(_, _) => {}
                },
            }
        }
        Ok(())
    }

    fn process_shutdown(&mut self) -> Result<(), InternalError> {
        match self.shutdown.try_recv() {
            Err(chan::TryRecvError::Empty) => Ok(()),
            Ok(()) => {
                // TODO: Disconnect all resources
                Ok(())
            }
            Err(chan::TryRecvError::Disconnected) => Err(InternalError::ShutdownChanelBroken),
        }
    }
}

#[derive(Debug, Display, Error)]
#[display(doc_comments)]
pub enum InternalError {
    /// shutdown channel in the reactor is broken
    ShutdownChanelBroken,

    /// control channel is broken; unable to send request
    ControlChannelBroken,
}

impl<R: Resource> From<chan::SendError<ControlEvent<R>>> for InternalError {
    fn from(_: chan::SendError<ControlEvent<R>>) -> Self {
        InternalError::ControlChannelBroken
    }
}

/// Handler is a business logic executed by the reactor. It is moved into a
/// dedicated reactor thread in [`Runtime`] and called upon reactor input events.
/// The handler object can't and should not be accessed from outside of the
/// reactor.
///
/// If the business logic needs to control the reactor it should save and use
/// [`Controller`] provided to it upon construction in [`Handler::new`].
pub trait Handler<R: Resource>: Send {
    type Context: Send;

    /// Constructs handler, providing reactor controller and additional data.
    ///
    /// Reactor controller is used by the handler to send the data to the resources,
    /// connect and disconnect resources.
    fn new(controller: Controller<R>, ctx: Self::Context) -> Self;

    /// Called by reactor dispatcher upon receiving message from the remote peer.
    fn received(&mut self, remote_addr: R::Addr, message: Box<[u8]>);

    /// Connection attempt underway.
    ///
    /// This is only encountered when an outgoing connection attempt is made,
    /// and is always called before [`Self::connected`].
    ///
    /// For incoming connections, [`Self::connected`] is called directly.
    fn attempted(&mut self, remote_addr: R::Addr);

    /// Called whenever a new connection with a peer is established, either
    /// inbound or outbound caused by [`Reactor::connect`] call. Indicates
    /// completion of the handshake protocol.
    fn connected(&mut self, remote_addr: R::Addr, direction: ConnDirection);

    /// Called whenever remote peer got disconnected, either because of the
    /// network event or due to a [`Reactor::disconnect`] call.
    fn disconnected(&mut self, remote_addr: R::Addr, reason: R::DisconnectReason);

    /// Called when event loop was interrupted because of the resource manager
    /// timeout.
    fn timeout(&mut self);

    /// Called upon resource-related error
    fn resource_error(&mut self, error: R::Error);

    /// Called upon internal (non-resource related) failure.
    fn internal_failure(&mut self, error: InternalError);

    /// Called by the reactor every time the event loop gets data from the network.
    ///
    /// Used to update the state machine's internal clock.
    ///
    /// "a regular short, sharp sound, especially that made by a clock or watch, typically
    /// every second."
    fn tick(&mut self);

    /// Called by the reactor after a timeout whenever an [`ReactorDispatch::SetTimer`]
    /// was received by the reactor from this iterator.
    ///
    /// NB: on each of this calls [`Self::tick`] is also called.
    fn on_timer(&mut self);
}

/// Direction of the resource connection.
#[derive(Clone, Copy, PartialEq, Eq, Ord, PartialOrd, Hash, Debug)]
pub enum ConnDirection {
    /// Inbound connection.
    Inbound,

    /// Outbound connection.
    Outbound,
}

/// Input events generated by the resources monitored in the reactor and used to
/// generate calls to the [`Handler`].
#[derive(Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Debug)]
pub enum InputEvent<R: Resource> {
    Spawn(R::Raw),

    Upgraded(),

    /// A connection to the resource was successfully established. Generated
    /// only when the handshake protocol was completed.
    Connected {
        remote_addr: R::Addr,
        direction: ConnDirection,
    },

    /// Resource was disconnected.
    Disconnected(R::Addr, R::DisconnectReason),

    /// A resource manager timed out on all resource read requests.
    Timeout,

    /// A timer set with [`ReactorApi::set_timer`] has expired.
    Timer,

    /// Data received from the resource
    Received(R::Addr, Box<[u8]>),
}

/// Events send by [`Controller`] and [`ReactorApi`] to the [`Runtime`].
#[derive(Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Debug)]
enum ControlEvent<R: Resource> {
    /// Request reactor to connect to the resource by address
    Connect(R::Addr),

    /// Request reactor to disconnect from a resource
    Disconnect(R::Addr),

    /// Ask reactor to wake up after certain interval
    SetTimer(),

    /// Request reactor to send the data to the resource
    Send(R::Addr, Vec<u8>),
}

/// Implements specific way of managing multiple resources for a reactor.
/// Blocks on concurrent events from multiple resources.
pub trait ResourceMgr<R: Resource>: Send {
    /// Iterator over input events returned by [`Self::read_events`].
    type EventIterator: Iterator<Item = InputEvent<R>>;

    /// Detects whether a given resource is known to the manager.
    fn has(&self, addr: &R::Addr) -> bool {
        self.get(addr).is_some()
    }

    /// Retrieves a resource reference by its address.
    fn get(&self, addr: &R::Addr) -> Option<&R>;

    /// Connects new resource and adds it to the manager.
    ///
    /// The implementations must make sure that [`InputEvent::Connected`] is
    /// emit upon successful connection.
    ///
    /// Must return false if the resource was already connected.
    fn connect_resource(&mut self, addr: &R::Addr) -> Result<bool, R::Error>;

    /// Disconnects resource and removes it from the manager.
    ///
    /// The implementations must make sure that [`IoEvent::Disconnected`] is
    /// emit upon successful disconnection.
    ///
    /// Must return false if the resource was already connected.
    fn disconnect_resource(&mut self, addr: &R::Addr) -> Result<bool, R::Error>;

    /// Adds already operating/connected resource to the manager. Returns `None`
    /// if the resource is already known, or a reference to the added resource.
    ///
    /// Should not generate any [`InputEvent`].
    fn register_resource(&mut self, resource: R) -> Option<&R>;

    /// Removes resource from the manager without disconnecting it or generating
    /// any events. Stops resource monitoring and returns the resource itself
    /// (like connection or a TCP stream). May be used later to insert resource
    /// back to the manager with [`Self::register_resource`].
    ///
    /// Should not generate any [`InputEvent`].
    fn unregister_resource(&mut self, addr: &R::Addr) -> Option<R>;

    /// Reads events from all resources under this manager and return an iterator
    /// for past and newly generated events. Blocks on the read operation or
    /// until the timeout.
    fn read_events(&mut self) -> Result<&mut Self::EventIterator, R::Error>;

    /// Returns iterator over events generated by all _previous_ calls to
    /// [`Self::read_events`] and which were not yet consumed.
    fn events(&mut self) -> &mut Self::EventIterator;

    /// Converts the self into an iterator over events generated by all
    /// _previous_ calls to [`Self::read_events`] and which were not yet consumed.
    fn into_events(self) -> Self::EventIterator;

    /// Sends data to the resource.
    fn send(&mut self, addr: &R::Addr, data: impl AsRef<[u8]>) -> Result<usize, R::Error>;
}
