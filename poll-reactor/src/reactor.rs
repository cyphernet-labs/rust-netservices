use std::collections::{hash_map, HashMap};
use std::os::unix::io::RawFd;
use std::thread::JoinHandle;

use crossbeam_channel as chan;

use crate::poller::Poll;
use crate::resource::Resource;
use crate::Error;

pub trait Service {
    type Listener: Resource;
    type Session: Resource;
    type Command;

    fn handle_listener_events(
        &mut self,
        id: <Self::Listener as Resource>::Id,
        resources: &mut Resources<Self>,
    ) -> Result<(), Error>;

    fn handle_session_events(
        &mut self,
        id: <Self::Session as Resource>::Id,
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

pub struct Runtime<S: Service, P: Poll> {
    service: S,
    poller: P,
    resources: Resources<S>,
    listener_map: HashMap<RawFd, <S::Listener as Resource>::Id>,
    session_map: HashMap<RawFd, <S::Session as Resource>::Id>,
}

impl<S: Service, P: Poll> Runtime<S, P> {
    pub fn run_loop(&mut self) -> Result<(), Error> {
        loop {
            self.poller.poll()?;
            for (fd, io) in &mut self.poller {
                if let Some(id) = self.listener_map.get(&fd) {
                    let count = {
                        let res = self
                            .resources
                            .listeners
                            .get_mut(id)
                            .expect("resource disappeared");
                        res.handle_io(io)
                            .map_err(|err| Error::Resource(Box::new(err)))?
                    };
                    if count > 0 {
                        self.service
                            .handle_listener_events(*id, &mut self.resources)?;
                    }
                }
            }
        }
    }
}

pub struct Resources<S: Service + ?Sized> {
    listeners: HashMap<<S::Listener as Resource>::Id, S::Listener>,
    sessions: HashMap<<S::Session as Resource>::Id, S::Session>,
    // waker
    // timeouts
}

impl<S: Service> Resources<S> {
    pub fn listen(&mut self, listener: S::Listener) -> Result<bool, Error> {
        todo!()
    }

    pub fn connect(&mut self, connection: S::Session) -> Result<bool, Error> {
        todo!()
    }

    pub fn disconnect(&mut self, session_id: <S::Session as Resource>::Id) -> Result<bool, Error> {
        todo!()
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
}
