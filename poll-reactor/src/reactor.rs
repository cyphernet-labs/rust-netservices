use std::collections::HashMap;
use std::io;
use std::os::unix::io::{AsRawFd, RawFd};
use std::thread::JoinHandle;
use std::time::Duration;

use crossbeam_channel as chan;

use crate::poller::Poll;
use crate::{Resource, ResourceId};

#[derive(Debug, Display, Error)]
#[display(doc_comments)]
pub enum Error<L: ResourceId, T: ResourceId> {
    /// unknown listener {0}
    ListenerUnknown(L),

    /// no connection with to peer {0}
    PeerUnknown(T),

    /// connection with peer {0} got broken
    PeerDisconnected(T, io::Error),
}

pub enum Action<L: Resource, T: Resource> {
    RegisterListener(L),
    RegisterTransport(T),
    UnregisterListener(L::Id),
    UnregisterTransport(T::Id),
    Send(T::Id, Vec<T::Message>),
}

pub trait Handler: Send + Iterator<Item = Action<Self::Listener, Self::Transport>> {
    type Listener: Resource;
    type Transport: Resource;
    type Command: Send;

    fn handle_wakeup(&mut self);

    fn handle_listener_event(
        &mut self,
        id: <Self::Listener as Resource>::Id,
        event: <Self::Listener as Resource>::Event,
        duration: Duration,
    );

    fn handle_transport_event(
        &mut self,
        id: <Self::Transport as Resource>::Id,
        event: <Self::Transport as Resource>::Event,
        duration: Duration,
    );

    fn handle_command(&mut self, cmd: Self::Command);

    fn handle_error(
        &mut self,
        err: Error<<Self::Listener as Resource>::Id, <Self::Transport as Resource>::Id>,
    );
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
                transports: empty!(),
                listener_map: empty!(),
                transport_map: empty!(),
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
    transport_map: HashMap<RawFd, <H::Transport as Resource>::Id>,
    listeners: HashMap<<H::Listener as Resource>::Id, H::Listener>,
    transports: HashMap<<H::Transport as Resource>::Id, H::Transport>,
    // waker
    // timeouts
}

impl<H: Handler, P: Poll> Runtime<H, P> {
    fn run(mut self) {
        loop {
            let (duration, count) = self.poller.poll();
            if count > 0 {
                self.handle_events(duration);
            }
            // TODO process commands
        }
    }

    fn handle_events(&mut self, duration: Duration) {
        for (fd, io) in &mut self.poller {
            if let Some(id) = self.listener_map.get(&fd) {
                let res = self.listeners.get_mut(id).expect("resource disappeared");
                res.handle_io(io);
                for event in res {
                    self.service.handle_listener_event(*id, event, duration);
                }
            } else if let Some(id) = self.transport_map.get(&fd) {
                let res = self.transports.get_mut(id).expect("resource disappeared");
                res.handle_io(io);
                for event in res {
                    self.service.handle_transport_event(*id, event, duration);
                }
            }
        }

        while let Some(action) = self.service.next() {
            // NB: Deadlock may happen here if the service will generate events over and over
            // in the handle_* calls we may never get out of this loop
        }
    }

    fn handle_action(
        &mut self,
        action: Action<H::Listener, H::Transport>,
    ) -> Result<(), Error<<H::Listener as Resource>::Id, <H::Transport as Resource>::Id>> {
        match action {
            Action::RegisterListener(listener) => {
                let id = listener.id();
                let fd = listener.as_raw_fd();
                self.poller.register(fd);
                self.listeners.insert(id, listener);
                self.listener_map.insert(fd, id);
            }
            Action::RegisterTransport(transport) => {
                let id = transport.id();
                let fd = transport.as_raw_fd();
                self.poller.register(fd);
                self.transports.insert(id, transport);
                self.transport_map.insert(fd, id);
            }
            Action::UnregisterListener(id) => {
                let listener = self
                    .listeners
                    .remove(&id)
                    .ok_or(Error::ListenerUnknown(id))?;
                let fd = listener.as_raw_fd();
                self.listener_map
                    .remove(&fd)
                    .expect("listener index content doesn't match registered listeners");
                self.poller.unregister(fd);
            }
            Action::UnregisterTransport(id) => {
                let transport = self.transports.remove(&id).ok_or(Error::PeerUnknown(id))?;
                let fd = transport.as_raw_fd();
                self.transport_map
                    .remove(&fd)
                    .expect("transport index content doesn't match registered transports");
                self.poller.unregister(fd);
            }
            Action::Send(id, msgs) => {
                let transport = self.transports.get_mut(&id).ok_or(Error::PeerUnknown(id))?;
                for msg in msgs {
                    // If we fail on sending any message this means disconnection (I/O write
                    // has failed for a given transport). We report error -- and lose all other
                    // messages we planned to send
                    transport
                        .send(msg)
                        .map_err(|err| Error::PeerDisconnected(id, err))?;
                }
            }
        }
        Ok(())
    }
}
