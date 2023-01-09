use std::collections::VecDeque;
use std::os::fd::RawFd;
use std::time::Instant;
use std::{io, net};

use cyphernet::crypto::ed25519::{PrivateKey, PublicKey};
use netservices::noise::NoiseXk;
use netservices::{ListenerEvent, NetSession, SessionEvent};
use reactor::{Error, Resource};

use crate::Transport;

pub type Accept = netservices::NetAccept<NoiseXk<PrivateKey>>;
pub type Action = reactor::Action<Accept, Transport>;
pub type NodeKeys = netservices::noise::NodeKeys<PrivateKey>;

pub trait Delegate: Default + Send {
    fn new_client(&mut self, id: RawFd, key: PublicKey) -> Vec<Action>;
    fn input(&mut self, id: RawFd, data: Vec<u8>, ecdh: &PrivateKey) -> Vec<Action>;
}

pub struct Server<D: Delegate> {
    action_queue: VecDeque<Action>,
    delegate: D,
    ecdh: PrivateKey,
}

impl<D: Delegate> Server<D> {
    pub fn bind(ecdh: PrivateKey, listen: net::SocketAddr) -> io::Result<Self> {
        let mut action_queue = VecDeque::new();
        let listener = Accept::bind(listen, ecdh.clone())?;
        action_queue.push_back(Action::RegisterListener(listener));
        Ok(Self {
            action_queue,
            delegate: D::default(),
            ecdh,
        })
    }
}

impl<D: Delegate> reactor::Handler for Server<D> {
    type Listener = Accept;
    type Transport = Transport;
    type Command = ();

    fn tick(&mut self, time: Instant) {
        let time = time.elapsed().as_micros();
        log::trace!(target: "server", "reactor ticks at {time}");
    }

    fn handle_wakeup(&mut self) {
        log::trace!(target: "server", "Reactor wakes up");
    }

    fn handle_listener_event(
        &mut self,
        id: <Self::Listener as Resource>::Id,
        event: <Self::Listener as Resource>::Event,
        time: Instant,
    ) {
        log::trace!(target: "server", "Listener event on {id} at {time:?}");
        match event {
            ListenerEvent::Accepted(session) => {
                log::info!(target: "server", "Incoming connection from {} on {}", session.transient_addr(), session.local_addr());
                match Transport::accept(session) {
                    Ok(transport) => {
                        log::info!(target: "server", "Connection accepted, registering {} with reactor", transport.transient_addr());
                        self.action_queue
                            .push_back(Action::RegisterTransport(transport));
                    }
                    Err(err) => {
                        log::info!(target: "server", "Error accepting incoming connection: {err}");
                    }
                }
            }
            ListenerEvent::Failure(err) => {
                log::error!(target: "server", "Error on listener {id}: {err}")
            }
        }
    }

    fn handle_transport_event(
        &mut self,
        id: <Self::Transport as Resource>::Id,
        event: <Self::Transport as Resource>::Event,
        time: Instant,
    ) {
        log::trace!(target: "server", "I/O on {id} at {time:?}");
        match event {
            SessionEvent::Established(key) => {
                log::debug!(target: "server", "Connection with remote peer {key}@{id} successfully established");
                self.action_queue.extend(self.delegate.new_client(id, key));
            }
            SessionEvent::Data(data) => {
                log::trace!(target: "server", "Incoming data {data:?}");
                self.action_queue
                    .extend(self.delegate.input(id, data, &self.ecdh));
            }
            SessionEvent::Terminated(err) => {
                log::error!(target: "server", "Connection with {id} is terminated due to an error: {err}");
            }
        }
    }

    fn handle_command(&mut self, cmd: Self::Command) {
        log::debug!(target: "server", "Command {cmd:?} received");
    }

    fn handle_error(&mut self, err: Error<Self::Listener, Self::Transport>) {
        match err {
            Error::TransportDisconnect(id, transport, _) => {
                log::warn!(target: "server", "Remote peer {}@{id} disconnected", transport.transient_addr());
                return;
            }
            // All others are errors:
            Error::ListenerUnknown(_) => {}
            Error::TransportUnknown(_) => {}
            Error::WriteFailure(_, _) => {}
            Error::ListenerDisconnect(_, _, _) => {}
            Error::ListenerPollError(_, _) => {}
            Error::TransportPollError(_, _) => {}
            Error::Poll(_) => {}
        }
        log::error!(target: "server", "Error: {err}");
    }

    fn handover_listener(&mut self, listener: Self::Listener) {
        log::error!(target: "server", "Disconnected listener socket {}", listener.id());
        panic!("Disconnected listener socket {}", listener.id())
    }

    fn handover_transport(&mut self, transport: Self::Transport) {
        log::warn!(target: "server", "Remote peer {}@{:?} disconnected", transport.transient_addr(), Resource::id(&transport));
    }
}

impl<D: Delegate> Iterator for Server<D> {
    type Item = Action;

    fn next(&mut self) -> Option<Self::Item> {
        self.action_queue.pop_front()
    }
}
