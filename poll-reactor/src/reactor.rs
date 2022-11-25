use std::collections::{hash_map, HashMap};
use std::thread::JoinHandle;

use crossbeam_channel as chan;

use crate::resource::{Event, Resource};
use crate::Error;

pub trait Service {
    type Listener: Resource;
    type Session: Resource;
    type Command;

    fn handle_listener_event(
        &mut self,
        id: <Self::Listener as Resource>::Id,
        event: Event<Self::Listener>,
        resources: &mut Resources<Self>,
    ) -> Result<(), Error>;

    fn handle_session_event(
        &mut self,
        id: <Self::Session as Resource>::Id,
        event: Event<Self::Session>,
        resources: &mut Resources<Self>,
    ) -> Result<(), Error>;

    fn handle_command(&mut self, cmd: Self::Command) -> Result<(), Error>;
}

pub struct Reactor<S: Service> {
    thread: JoinHandle<()>,
    control_send: chan::Sender<S::Command>,
    control_recv: chan::Receiver<S::Command>,
    shutdown_send: chan::Sender<()>,
    shutdown_recv: chan::Receiver<()>,
}

pub struct Runtime<S: Service> {
    service: S,
    resources: Resources<S>,
}

impl<S: Service> Runtime<S> {
    pub fn run_loop(&mut self) -> Result<(), Error> {
        todo!()
    }
}

pub struct Resources<S: Service + ?Sized> {
    listeners: HashMap<<S::Listener as Resource>::Id, S::Listener>,
    sessions: HashMap<<S::Session as Resource>::Id, S::Session>,
    // waker
    // timeouts
}

impl<S: Service> Resources<S> {
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

    pub fn listener_mut(&mut self, id: <S::Listener as Resource>::Id) -> Option<&mut S::Listener> {
        self.listeners.get_mut(&id)
    }

    pub fn session_mut(&mut self, id: <S::Session as Resource>::Id) -> Option<&mut S::Session> {
        self.sessions.get_mut(&id)
    }
}
