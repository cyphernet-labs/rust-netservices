use std::collections::{hash_map, HashMap};
use std::os::unix::io::RawFd;
use std::thread::JoinHandle;

use crossbeam_channel as chan;

use crate::poller::Poll;
use crate::resource::{Resource, ResourceId};

pub trait Service: Send {
    type Listener: Resource;
    type Session: Resource;
    type Command: Send;

    fn handle_listener_events(
        &mut self,
        id: <Self::Listener as Resource>::Id,
        resources: &mut Resources<Self>,
    );

    fn handle_session_events(
        &mut self,
        id: <Self::Session as Resource>::Id,
        resources: &mut Resources<Self>,
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
                resources: Resources::new(),
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
    resources: Resources<S>,
    listener_map: HashMap<RawFd, <S::Listener as Resource>::Id>,
    session_map: HashMap<RawFd, <S::Session as Resource>::Id>,
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
                let count = {
                    let res = self
                        .resources
                        .listeners
                        .get_mut(id)
                        .expect("resource disappeared");
                    res.handle_io(io)
                };
                if count > 0 {
                    self.service
                        .handle_listener_events(*id, &mut self.resources);
                }
            }
        }
    }
}

#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Debug, Display, Error)]
#[display("the resource with the same ID {0} is already registered")]
pub struct AlreadyRegistered<Id: ResourceId>(Id);

#[derive(Debug)]
pub struct Resources<S: Service + ?Sized> {
    listeners: HashMap<<S::Listener as Resource>::Id, S::Listener>,
    sessions: HashMap<<S::Session as Resource>::Id, S::Session>,
    // waker
    // timeouts
}

impl<S: Service> Resources<S> {
    fn new() -> Self {
        Self {
            listeners: empty!(),
            sessions: empty!(),
        }
    }

    pub fn listeners(&self) -> hash_map::Iter<<S::Listener as Resource>::Id, S::Listener> {
        self.listeners.iter()
    }

    pub fn sessions(&self) -> hash_map::Iter<<S::Session as Resource>::Id, S::Session> {
        self.sessions.iter()
    }

    pub fn listener(&self, id: <S::Listener as Resource>::Id) -> Option<&S::Listener> {
        self.listeners.get(&id)
    }

    pub fn session(&self, id: <S::Session as Resource>::Id) -> Option<&S::Session> {
        self.sessions.get(&id)
    }

    pub fn register_listener(
        &mut self,
        listener: S::Listener,
    ) -> Result<(), AlreadyRegistered<<S::Listener as Resource>::Id>> {
        todo!()
    }

    pub fn unregister_listener(
        &mut self,
        id: <S::Listener as Resource>::Id,
    ) -> Option<S::Listener> {
        todo!()
    }

    pub fn register_session(
        &mut self,
        session: S::Session,
    ) -> Result<(), AlreadyRegistered<<S::Session as Resource>::Id>> {
        todo!()
    }

    pub fn unregister_session(&mut self, id: <S::Session as Resource>::Id) -> Option<S::Session> {
        todo!()
    }
}
