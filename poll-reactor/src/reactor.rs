use amplify::confinement::Confined;
use std::collections::HashMap;
use std::os::unix::io::RawFd;
use std::thread::JoinHandle;

use crossbeam_channel as chan;

use crate::poller::Poll;
use crate::resource::{Resource, ResourceId};

pub enum ServiceEvent<L: Resource, C: Resource, const MAX_SIZE: usize> {
    RegisterListener(L),
    RegisterService(C),
    UnregisterListener(L),
    UnregisterService(C),
    Send(C::Id, Confined<Vec<u8>, 1, MAX_SIZE>),
}

pub trait Service<const MAX_DATA_SIZE: usize = { usize::MAX }>:
    Send + Iterator<Item = ServiceEvent<Self::Listener, Self::Session, MAX_DATA_SIZE>>
{
    type Listener: Resource;
    type Session: Resource;
    type Command: Send;

    fn handle_listener_event(
        &mut self,
        id: <Self::Listener as Resource>::Id,
        event: <Self::Listener as Resource>::Event,
    );

    fn handle_session_event(
        &mut self,
        id: <Self::Session as Resource>::Id,
        event: <Self::Session as Resource>::Event,
    );

    fn handle_command(&mut self, cmd: Self::Command);
}

pub struct Reactor<S: Service> {
    thread: JoinHandle<()>,
    control_send: chan::Sender<S::Command>,
    shutdown_send: chan::Sender<()>,
}

impl<S: Service> Reactor<S> {
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
                sessions: empty!(),
                listener_map: empty!(),
                session_map: empty!(),
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

pub struct Runtime<S: Service, P: Poll> {
    service: S,
    poller: P,
    control_recv: chan::Receiver<S::Command>,
    shutdown_recv: chan::Receiver<()>,
    listener_map: HashMap<RawFd, <S::Listener as Resource>::Id>,
    session_map: HashMap<RawFd, <S::Session as Resource>::Id>,
    listeners: HashMap<<S::Listener as Resource>::Id, S::Listener>,
    sessions: HashMap<<S::Session as Resource>::Id, S::Session>,
    // waker
    // timeouts
}

impl<S: Service, P: Poll> Runtime<S, P> {
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
            } else if let Some(id) = self.session_map.get(&fd) {
                let res = self.sessions.get_mut(id).expect("resource disappeared");
                res.handle_io(io);
                for event in res {
                    self.service.handle_session_event(*id, event);
                }
            }
        }

        while let Some(event) = self.service.next() {
            match event {
                ServiceEvent::RegisterListener(_) => {}
                ServiceEvent::RegisterService(_) => {}
                ServiceEvent::UnregisterListener(_) => {}
                ServiceEvent::UnregisterService(_) => {}
                ServiceEvent::Send(id, _) => {}
            }
        }
    }
}

#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Debug, Display, Error)]
#[display("the resource with the same ID {0} is already registered")]
pub struct AlreadyRegistered<Id: ResourceId>(Id);
