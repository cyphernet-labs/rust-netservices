use amplify::confinement::Confined;
use std::collections::HashMap;
use std::os::unix::io::RawFd;
use std::thread::JoinHandle;

use crossbeam_channel as chan;

use crate::poller::Poll;
use crate::resource::{Resource, ResourceId};

pub enum Action<L: Resource, C: Resource, const MAX_SIZE: usize> {
    RegisterListener(L),
    RegisterConnection(C),
    UnregisterListener(L),
    UnregisterConnection(C),
    Send(C::Id, Confined<Vec<u8>, 1, MAX_SIZE>),
}

pub trait Handler<const MAX_DATA_SIZE: usize = { usize::MAX }>:
    Send + Iterator<Item = Action<Self::Listener, Self::Connection, MAX_DATA_SIZE>>
{
    type Listener: Resource;
    type Connection: Resource;
    type Command: Send;

    fn handle_listener_event(
        &mut self,
        id: <Self::Listener as Resource>::Id,
        event: <Self::Listener as Resource>::Event,
    );

    fn handle_connection_event(
        &mut self,
        id: <Self::Connection as Resource>::Id,
        event: <Self::Connection as Resource>::Event,
    );

    fn handle_command(&mut self, cmd: Self::Command);
}

pub struct Reactor<S: Handler> {
    thread: JoinHandle<()>,
    control_send: chan::Sender<S::Command>,
    shutdown_send: chan::Sender<()>,
}

impl<S: Handler> Reactor<S> {
    pub fn new<P: Poll>(service: S, poller: P) -> Self
    where
        S: 'static,
        P: 'static,
    {
        let (shutdown_send, shutdown_recv) = chan::bounded(1);
        let (control_send, control_recv) = chan::unbounded();

        let thread = std::thread::spawn(move || {
            let runtime = Runtime {
                service,
                poller,
                control_recv,
                shutdown_recv,
                listeners: empty!(),
                connections: empty!(),
                listener_map: empty!(),
                connection_map: empty!(),
            };

            runtime.run();
        });

        Self {
            thread,
            control_send,
            shutdown_send,
        }
    }
}

pub struct Runtime<H: Handler, P: Poll> {
    service: H,
    poller: P,
    control_recv: chan::Receiver<H::Command>,
    shutdown_recv: chan::Receiver<()>,
    listener_map: HashMap<RawFd, <H::Listener as Resource>::Id>,
    connection_map: HashMap<RawFd, <H::Connection as Resource>::Id>,
    listeners: HashMap<<H::Listener as Resource>::Id, H::Listener>,
    connections: HashMap<<H::Connection as Resource>::Id, H::Connection>,
    // waker
    // timeouts
}

impl<H: Handler, P: Poll> Runtime<H, P> {
    fn run(mut self) {
        loop {
            if self.poller.poll() > 0 {
                self.handle_events();
            }
        }
    }

    fn handle_events(&mut self) {
        for (fd, io) in &mut self.poller {
            if let Some(id) = self.listener_map.get(&fd) {
                let res = self.listeners.get_mut(id).expect("resource disappeared");
                res.handle_io(io);
                for event in res {
                    self.service.handle_listener_event(*id, event);
                }
            } else if let Some(id) = self.connection_map.get(&fd) {
                let res = self.connections.get_mut(id).expect("resource disappeared");
                res.handle_io(io);
                for event in res {
                    self.service.handle_connection_event(*id, event);
                }
            }
        }

        while let Some(action) = self.service.next() {
            match action {
                Action::RegisterListener(_) => {}
                Action::RegisterConnection(_) => {}
                Action::UnregisterListener(_) => {}
                Action::UnregisterConnection(_) => {}
                Action::Send(id, _) => {}
            }
        }
    }
}

#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Debug, Display, Error)]
#[display("the resource with the same ID {0} is already registered")]
pub struct AlreadyRegistered<Id: ResourceId>(Id);
