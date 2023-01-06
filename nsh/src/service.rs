use cyphernet::crypto::ed25519::PrivateKey;
use ed25519_compact::x25519::PublicKey;
use netservices::noise::NoiseXk;
use reactor::{Error, Resource};
use std::collections::VecDeque;
use std::net;
use std::time::Instant;

pub type NetTransport = netservices::NetTransport<NoiseXk<PrivateKey>>;
pub type NetAccept = netservices::NetAccept<NoiseXk<PrivateKey>>;
pub type Action = reactor::Action<NetAccept, NetTransport>;
pub type NodeKeys = netservices::noise::NodeKeys<PrivateKey>;

pub struct Service {
    action_queue: VecDeque<Action>,
}

impl Service {
    pub fn new(node_keys: NodeKeys, listen: net::SocketAddr) -> Self {
        Self {
            action_queue: empty!(),
        }
    }
}

impl reactor::Handler for Service {
    type Listener = NetAccept;
    type Transport = NetTransport;
    type Command = ();

    fn tick(&mut self, time: Instant) {
        todo!()
    }

    fn handle_wakeup(&mut self) {
        todo!()
    }

    fn handle_listener_event(
        &mut self,
        id: <Self::Listener as Resource>::Id,
        event: <Self::Listener as Resource>::Event,
        time: Instant,
    ) {
        todo!()
    }

    fn handle_transport_event(
        &mut self,
        id: <Self::Transport as Resource>::Id,
        event: <Self::Transport as Resource>::Event,
        time: Instant,
    ) {
        todo!()
    }

    fn handle_command(&mut self, cmd: Self::Command) {
        todo!()
    }

    fn handle_error(
        &mut self,
        err: Error<<Self::Listener as Resource>::Id, <Self::Transport as Resource>::Id>,
    ) {
        todo!()
    }

    fn handover_listener(&mut self, listener: Self::Listener) {
        todo!()
    }

    fn handover_transport(&mut self, transport: Self::Transport) {
        todo!()
    }
}

impl Iterator for Service {
    type Item = Action;

    fn next(&mut self) -> Option<Self::Item> {
        todo!()
    }
}
