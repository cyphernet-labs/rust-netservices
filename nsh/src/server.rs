use std::collections::VecDeque;
use std::process::{Command, Stdio};
use std::time::Instant;
use std::{io, net};

use cyphernet::crypto::ed25519::PrivateKey;
use netservices::noise::NoiseXk;
use netservices::{ListenerEvent, NetSession, SessionEvent};
use reactor::{Error, Resource};

use crate::Transport;

pub type Accept = netservices::NetAccept<NoiseXk<PrivateKey>>;
pub type Action = reactor::Action<Accept, Transport>;
pub type NodeKeys = netservices::noise::NodeKeys<PrivateKey>;

pub struct Server {
    action_queue: VecDeque<Action>,
}

impl Server {
    pub fn bind(ecdh: PrivateKey, listen: net::SocketAddr) -> io::Result<Self> {
        let mut action_queue = VecDeque::new();
        let listener = Accept::bind(listen, ecdh)?;
        action_queue.push_back(Action::RegisterListener(listener));
        Ok(Self { action_queue })
    }
}

impl reactor::Handler for Server {
    type Listener = Accept;
    type Transport = Transport;
    type Command = ();

    fn tick(&mut self, time: Instant) {
        let time = time.elapsed().as_micros();
        log::trace!(target: "nsh", "reactor ticks at {time}");
    }

    fn handle_wakeup(&mut self) {
        log::trace!(target: "nsh", "Reactor wakes up");
    }

    fn handle_listener_event(
        &mut self,
        id: <Self::Listener as Resource>::Id,
        event: <Self::Listener as Resource>::Event,
        time: Instant,
    ) {
        log::trace!(target: "nsh", "Listener event on {id} at {time:?}");
        match event {
            ListenerEvent::Accepted(session) => {
                log::info!(target: "nsh", "Incoming connection from {} on {}", session.transient_addr(), session.local_addr());
                match Transport::accept(session) {
                    Ok(transport) => {
                        log::info!(target: "nsh", "Connection accepted, registering {} with reactor", transport.transient_addr());
                        self.action_queue
                            .push_back(Action::RegisterTransport(transport));
                    }
                    Err(err) => {
                        log::info!(target: "nsh", "Error accepting incoming connection: {err}");
                    }
                }
            }
            ListenerEvent::Failure(err) => {
                log::error!(target: "nsh", "Error on listener {id}: {err}")
            }
        }
    }

    fn handle_transport_event(
        &mut self,
        id: <Self::Transport as Resource>::Id,
        event: <Self::Transport as Resource>::Event,
        time: Instant,
    ) {
        log::trace!(target: "nsh", "I/O on {id} at {time:?}");
        match event {
            SessionEvent::Established(key) => {
                log::info!(target: "nsh", "Connection with remote peer {key}@{id} successfully established");
            }
            SessionEvent::Data(data) => {
                log::trace!(target: "nsh", "Incoming data {data:?}");
                let cmd = match String::from_utf8(data) {
                    Ok(cmd) => {
                        log::info!(target: "nsh", "Executing `{cmd}` for {id}");
                        cmd
                    }
                    Err(err) => {
                        log::warn!(target: "nsh", "Non-UTF8 command from {id}: {err}");
                        self.action_queue
                            .push_back(Action::Send(id, b"NON_UTF8_COMMAND".to_vec()));
                        self.action_queue.push_back(Action::UnregisterTransport(id));
                        return;
                    }
                };
                match Command::new(cmd).stdout(Stdio::piped()).output() {
                    Ok(output) => {
                        log::debug!(target: "nsh", "Command executed successfully; {} bytes of output collected", output.stdout.len());
                        self.action_queue.push_back(Action::Send(id, output.stdout));
                        self.action_queue.push_back(Action::UnregisterTransport(id));
                    }
                    Err(err) => {
                        log::error!(target: "nsh", "Error executing command: {err}");
                        self.action_queue
                            .push_back(Action::Send(id, err.to_string().as_bytes().to_vec()));
                        self.action_queue.push_back(Action::UnregisterTransport(id));
                    }
                }
            }
            SessionEvent::Terminated(err) => {
                log::error!(target: "nsh", "Connection with {id} is terminated due to an error {err}");
            }
        }
    }

    fn handle_command(&mut self, cmd: Self::Command) {
        log::debug!(target: "nsh", "Command {cmd:?} received");
    }

    fn handle_error(
        &mut self,
        err: Error<<Self::Listener as Resource>::Id, <Self::Transport as Resource>::Id>,
    ) {
        log::error!(target: "nsh", "Error {err}");
    }

    fn handover_listener(&mut self, listener: Self::Listener) {
        log::error!(target: "nsh", "Disconnected listener socket {}", listener.id());
        panic!("Disconnected listener socket {}", listener.id())
    }

    fn handover_transport(&mut self, transport: Self::Transport) {
        log::warn!(target: "nsh", "Remote peer {}@{:?} disconnected", transport.transient_addr(), Resource::id(&transport));
    }
}

impl Iterator for Server {
    type Item = Action;

    fn next(&mut self) -> Option<Self::Item> {
        self.action_queue.pop_front()
    }
}
