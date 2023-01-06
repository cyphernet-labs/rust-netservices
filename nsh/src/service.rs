use std::collections::VecDeque;
use std::time::Instant;
use std::{io, net};

use cyphernet::crypto::ed25519::PrivateKey;
use netservices::noise::NoiseXk;
use netservices::{ListenerEvent, NetSession, SessionEvent};
use reactor::{Error, Resource};

use crate::NetTransport;

pub type NetAccept = netservices::NetAccept<NoiseXk<PrivateKey>>;
pub type Action = reactor::Action<NetAccept, NetTransport>;
pub type NodeKeys = netservices::noise::NodeKeys<PrivateKey>;

pub struct Service {
    action_queue: VecDeque<Action>,
}

impl Service {
    pub fn bind(ecdh: PrivateKey, listen: net::SocketAddr) -> io::Result<Self> {
        let mut action_queue = VecDeque::new();
        let listener = NetAccept::bind(listen, ecdh)?;
        action_queue.push_back(Action::RegisterListener(listener));
        Ok(Self { action_queue })
    }
}

impl reactor::Handler for Service {
    type Listener = NetAccept;
    type Transport = NetTransport;
    type Command = ();

    fn tick(&mut self, time: Instant) {
        let time = time.elapsed().as_micros();
        log::trace!(target: "transport", "[{time}] reactor ticks");
    }

    fn handle_wakeup(&mut self) {
        log::trace!(target: "transport", "Reactor wakes up");
    }

    fn handle_listener_event(
        &mut self,
        id: <Self::Listener as Resource>::Id,
        event: <Self::Listener as Resource>::Event,
        time: Instant,
    ) {
        let time = time.elapsed().as_micros();
        log::trace!(target: "transport", "[{time}] listener event on {id}");
        match event {
            ListenerEvent::Accepted(session) => {
                log::info!(target: "transport", "Incoming connection from {} on {}", session.transient_addr(), session.local_addr());
            }
            ListenerEvent::Failure(err) => {
                log::error!(target: "transport", "Error on listener {id}: {err}")
            }
        }
    }

    fn handle_transport_event(
        &mut self,
        id: <Self::Transport as Resource>::Id,
        event: <Self::Transport as Resource>::Event,
        time: Instant,
    ) {
        let time = time.elapsed().as_micros();
        log::trace!(target: "transport", "[{time}] transport event on {id}");
        match event {
            SessionEvent::Established(key) => {
                log::info!(target: "transport", "Connection with remote peer {key}@{id} successfully established")
            }
            SessionEvent::Data(data) => {
                log::trace!(target: "transport", "incoming data {data:?}")
            }
            SessionEvent::Terminated(err) => {
                log::error!(target: "transport", "Connection with {id} is terminated due to an error {err}")
            }
        }
    }

    fn handle_command(&mut self, cmd: Self::Command) {
        log::debug!(target: "transport", "Command {cmd:?} received");
    }

    fn handle_error(
        &mut self,
        err: Error<<Self::Listener as Resource>::Id, <Self::Transport as Resource>::Id>,
    ) {
        log::error!(target: "transport", "Error {err}");
    }

    fn handover_listener(&mut self, listener: Self::Listener) {
        log::error!(target: "transport", "Disconnected listener socket {}", listener.id());
        panic!("Disconnected listener socket {}", listener.id())
    }

    fn handover_transport(&mut self, transport: Self::Transport) {
        log::warn!(target: "transport", "Remote peer {}@{:?} disconnected", transport.transient_addr(), Resource::id(&transport));
    }
}

impl Iterator for Service {
    type Item = Action;

    fn next(&mut self) -> Option<Self::Item> {
        self.action_queue.pop_front()
    }
}
