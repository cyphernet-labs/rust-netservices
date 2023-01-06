use cyphernet::crypto::ed25519::PrivateKey;
use netservices::noise::NoiseXk;
use reactor::{Error, Resource};
use std::collections::VecDeque;
use std::time::Instant;
use std::{io, net};

pub type NetTransport = netservices::NetTransport<NoiseXk<PrivateKey>>;
pub type NetAccept = netservices::NetAccept<NoiseXk<PrivateKey>>;
pub type Action = reactor::Action<NetAccept, NetTransport>;
pub type NodeKeys = netservices::noise::NodeKeys<PrivateKey>;

pub struct Service {
    action_queue: VecDeque<Action>,
}

impl Service {
    pub fn with(ecdh: PrivateKey, listen: net::SocketAddr) -> io::Result<Self> {
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
