// Library for building scalable privacy-preserving microservices P2P nodes
//
// SPDX-License-Identifier: Apache-2.0
//
// Written in 2022-2024 by
//     Alexis Sellier <cloudhead@radicle.xyz>
//     Dr. Maxim Orlovsky <orlovsky@cyphernet.org>
//
// Copyright 2022-2024 Cyphernet Labs, IDCS, Switzerland
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::any::Any;
use std::collections::hash_map::Entry;
use std::collections::{HashMap, VecDeque};
use std::io::Write;
use std::os::fd::{AsRawFd, RawFd};
use std::sync::Arc;
use std::{io, net};

use cyphernet::addr::Addr;
use reactor::poller::popol;
use reactor::{Action, Error, Reactor, Resource, ResourceId, ResourceType, Timestamp};

use super::{DisconnectReason, Inbound, Outbound, Remote, Remotes};
use crate::{
    Artifact, Direction, Frame, ListenerEvent, NetAccept, NetConnection, NetListener, NetSession,
    NetTransport, SessionEvent,
};

#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub enum NodeCtl {
    /// Close all connections, unbind listeners, stop the reactor loop and complete the reactor
    /// thread.
    Terminate,
}

// TODO: Do a proper metrics measurements
// TODO: Consider collecting metrics using Marshaller; move (dis)connection counting to business
//       logic
#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug, Default)]
pub struct Metrics {
    pub bytes_sent: usize,
    pub bytes_received: usize,
    pub messages_sent: usize,
    pub messages_received: usize,
    pub inbound_connection_attempts: usize,
    pub outbound_connection_attempts: usize,
    pub disconnects: usize,
}

pub trait NodeController<A: Addr + Send, S: NetSession, L: NetListener<Stream = S::Connection>>:
    Send
{
    type Frame: Frame;

    // TODO: Replace with passing sender object
    fn extract_actions(
        &mut self,
    ) -> impl IntoIterator<Item = Action<NetAccept<S, L>, NetTransport<S>>>;

    fn should_accept(&mut self, remote: &A, time: Timestamp) -> bool;

    fn accept(
        &mut self,
        remote: A,
        connection: S::Connection,
        time: Timestamp,
    ) -> Result<S, impl std::error::Error>;

    fn on_listening(&mut self, socket: net::SocketAddr);

    fn on_listener_failure(&mut self, res_id: ResourceId, err: io::Error, time: Timestamp);

    fn on_established(
        &mut self,
        remote_id: <S::Artifact as Artifact>::NodeId,
        addr: A,
        direction: Direction,
        time: Timestamp,
    );

    fn on_disconnected(
        &mut self,
        remote_id: <S::Artifact as Artifact>::NodeId,
        direction: Direction,
        reason: &DisconnectReason,
    );

    /// Called when a listener get disconnected and handed over by the reactor
    fn on_unbound(&mut self, listener: NetAccept<S, L>);

    fn on_tick(
        &mut self,
        time: Timestamp,
        metrics: &HashMap<<S::Artifact as Artifact>::NodeId, Metrics>,
    );

    fn on_command(&mut self, cmd: NodeCtl);

    fn on_timer(&mut self);

    // TODO: Pass sender
    fn on_frame(&mut self, req: Self::Frame);

    /// Called on failure of frame parsing, before disconnecting the remote and calling
    /// [`on_disconnect`].
    fn on_frame_unparsable(&mut self, err: &<Self::Frame as Frame>::Error);
}

struct NodeService<
    S: NetSession,
    L: NetListener<Stream = S::Connection>,
    C: NodeController<<S::Connection as NetConnection>::Addr, S, L>,
> {
    /// Id of this node.
    local_id: <S::Artifact as Artifact>::NodeId,
    /// Server message processor implementing server business logic.
    controller: C,
    listening: HashMap<RawFd, net::SocketAddr>,
    inbound: HashMap<RawFd, Inbound<<S::Connection as NetConnection>::Addr>>,
    outbound: HashMap<
        RawFd,
        Outbound<<S::Connection as NetConnection>::Addr, <S::Artifact as Artifact>::NodeId>,
    >,
    remotes: Remotes<<S::Connection as NetConnection>::Addr, <S::Artifact as Artifact>::NodeId>,
    metrics: HashMap<<S::Artifact as Artifact>::NodeId, Metrics>,
    /// Buffer for the actions which has to be delivered to the reactor when it calls the
    /// [`NodeService`] as an iterator.
    actions: VecDeque<Action<NetAccept<S, L>, NetTransport<S>>>,
}

impl<
        S: NetSession,
        L: NetListener<Stream = S::Connection>,
        C: NodeController<<S::Connection as NetConnection>::Addr, S, L>,
    > NodeService<S, L, C>
{
    pub fn new(node_id: <S::Artifact as Artifact>::NodeId, controller: C) -> Self {
        NodeService {
            local_id: node_id,
            controller,
            listening: empty!(),
            inbound: empty!(),
            outbound: empty!(),
            remotes: empty!(),
            metrics: empty!(),
            actions: empty!(),
        }
    }

    pub fn listen(&mut self, socket: NetAccept<S, L>) {
        self.listening.insert(socket.as_raw_fd(), socket.local_addr());
        self.actions.push_back(Action::RegisterListener(socket));
    }

    fn disconnect(
        &mut self,
        res_id: ResourceId,
        reason: DisconnectReason,
    ) -> Option<(<S::Artifact as Artifact>::NodeId, Direction)> {
        match self.remotes.entry(res_id) {
            Entry::Vacant(_) => {
                // Connecting remote with no session.
                #[cfg(feature = "log")]
                log::debug!(target: "node-service", "Disconnecting pending remote with id={res_id}: {reason}");
                self.actions.push_back(Action::UnregisterTransport(res_id));

                // Check for attempted outbound connections. Unestablished inbound connections don't
                // have an ID yet.
                self.outbound
                    .values()
                    .find(|o| o.res_id == Some(res_id))
                    .map(|o| (o.remote_id.clone(), Direction::Outbound))
            }
            Entry::Occupied(mut entry) => match entry.get() {
                Remote::Disconnecting {
                    remote_id,
                    direction,
                    ..
                } => {
                    #[cfg(feature = "log")]
                    log::error!(target: "node-service", "Remote with id={res_id} is already disconnecting");

                    remote_id.as_ref().map(|id| (id.clone(), *direction))
                }
                Remote::Connected {
                    remote_id,
                    direction,
                    ..
                } => {
                    #[cfg(feature = "log")]
                    log::debug!(target: "node-service", "Disconnecting remote with id={res_id}: {reason}");

                    let direction = *direction;
                    let remote_id = *remote_id;

                    entry.insert(Remote::Disconnecting {
                        remote_id: Some(remote_id),
                        direction,
                        reason,
                    });
                    self.actions.push_back(Action::UnregisterTransport(res_id));

                    Some((remote_id, direction))
                }
            },
        }
    }

    fn cleanup(&mut self, res_id: ResourceId, fd: RawFd) {
        if self.inbound.remove(&fd).is_some() {
            #[cfg(feature = "log")]
            log::debug!(target: "node-service", "Cleaning up inbound remote state with id={res_id} (fd={fd})");
        } else if let Some(outbound) = self.outbound.remove(&fd) {
            #[cfg(feature = "log")]
            log::debug!(target: "node-service", "Cleaning up outbound remote state with id={res_id} (fd={fd})");
            self.controller.on_disconnected(
                outbound.remote_id,
                Direction::Outbound,
                &DisconnectReason::connection(),
            );
        } else {
            #[cfg(feature = "log")]
            log::warn!(target: "node-service", "Tried to cleanup unknown remote with id={res_id} (fd={fd})");
        }
    }

    /// Adds terminate action to the action queue, such that on the next reactor event loop run it
    /// will receive it and perform the termination procedure.
    fn terminate(&mut self) {
        #[cfg(feature = "log")]
        log::info!(target: "node-service", "Scheduling to terminate the reactor and client service");

        // TODO: Add correct termination: close all connections and unbind listeners

        self.actions.push_back(Action::Terminate);
    }
}

impl<
        S: NetSession,
        L: NetListener<Stream = S::Connection>,
        C: NodeController<<S::Connection as NetConnection>::Addr, S, L>,
    > reactor::Handler for NodeService<S, L, C>
{
    type Listener = NetAccept<S, L>;
    type Transport = NetTransport<S>;
    type Command = NodeCtl;

    fn tick(&mut self, time: Timestamp) { self.controller.on_tick(time, &self.metrics) }

    fn handle_timer(&mut self) { self.controller.on_timer() }

    fn handle_listener_event(
        &mut self,
        res_id: ResourceId,
        event: <Self::Listener as Resource>::Event,
        time: Timestamp,
    ) {
        match event {
            ListenerEvent::Accepted(connection) => {
                let Ok(remote) = connection.remote_addr() else {
                    #[cfg(feature = "log")]
                    log::warn!(target: "node-service", "Accepted connection doesn't have remote address; dropping");
                    drop(connection);

                    return;
                };
                let fd = connection.as_raw_fd();
                #[cfg(feature = "log")]
                log::debug!(target: "node-service", "Inbound connection from {remote} (fd={fd})");

                // If the service doesn't want to accept this connection,
                // we drop the connection here, which disconnects the socket.
                if !self.controller.should_accept(&remote, time) {
                    #[cfg(feature = "log")]
                    log::debug!(target: "node-service", "Rejecting inbound connection from {remote} (fd={fd})");
                    drop(connection);

                    return;
                }

                let session = match self.controller.accept(remote.clone(), connection, time) {
                    Ok(s) => s,
                    Err(err) => {
                        #[cfg(feature = "log")]
                        log::error!(target: "node-service", "Error creating session for {remote}: {err}");
                        return;
                    }
                };
                let transport = match NetTransport::with_session(session, Direction::Inbound) {
                    Ok(transport) => transport,
                    Err(err) => {
                        #[cfg(feature = "log")]
                        log::error!(target: "node-service", "Failed to create transport for accepted connection: {err}");
                        return;
                    }
                };
                #[cfg(feature = "log")]
                log::debug!(target: "node-service", "Accepted inbound connection from {remote} (fd={fd})");

                self.inbound.insert(fd, Inbound {
                    res_id: None,
                    addr: remote.clone(),
                });
                self.actions.push_back(Action::RegisterTransport(transport))
            }
            ListenerEvent::Failure(err) => {
                #[cfg(feature = "log")]
                log::error!(target: "node-service", "Error listening for inbound connections: {err}");
                self.controller.on_listener_failure(res_id, err, time);
            }
        }
    }

    fn handle_transport_event(
        &mut self,
        res_id: ResourceId,
        event: <Self::Transport as Resource>::Event,
        time: Timestamp,
    ) {
        match event {
            SessionEvent::Established(fd, artifact) => {
                // SAFETY: With the NoiseXK protocol, there is always a remote static key.
                let remote_id = artifact
                    .remote_id()
                    .expect("remote node ID must be known when the session is established");
                // Make sure we don't try to connect to ourselves by mistake.
                if remote_id == self.local_id {
                    #[cfg(feature = "log")]
                    log::error!(target: "node-service", "Self-connection detected, disconnecting");
                    self.disconnect(res_id, DisconnectReason::SelfConnection);

                    return;
                }
                let (addr, direction) = if let Some(remote) = self.inbound.remove(&fd) {
                    self.metrics.entry(remote_id).or_default().inbound_connection_attempts += 1;
                    (remote.addr, Direction::Inbound)
                } else if let Some(remote) = self.outbound.remove(&fd) {
                    assert_eq!(remote_id, remote.remote_id);
                    (remote.addr, Direction::Outbound)
                } else {
                    #[cfg(feature = "log")]
                    log::error!(target: "node-service", "Session for {remote_id} (id={res_id}) not found");
                    return;
                };
                #[cfg(feature = "log")]
                log::debug!(
                    target: "node-service",
                    "Session established with {remote_id} (id={res_id}, fd={fd}, {direction})",
                );

                // Connections to close.
                let mut disconnect = Vec::new();

                // Handle conflicting connections.
                // This is typical when nodes have mutually configured their nodes to connect to
                // each other on startup. We handle this by deterministically choosing one node
                // whos outbound connection is the one that is kept. The other connections are
                // dropped.
                {
                    // Whether we have precedence in case of conflicting connections.
                    // Having precedence means that our outbound connection will win over
                    // the other node's outbound connection.
                    let precedence = self.local_id > remote_id;

                    // Pre-existing connections that conflict with this newly established session.
                    // Note that we can't know whether a connection is conflicting before we get the
                    // remote static key.
                    let mut conflicting = Vec::new();

                    // Active sessions with the same remote node ID but a different Resource ID are
                    // conflicting.
                    conflicting.extend(
                        self.remotes
                            .active()
                            .filter(|(c_id, d, _)| **d == remote_id && *c_id != res_id)
                            .map(|(c_id, _, direction)| (c_id, direction)),
                    );

                    // Outbound connection attempts with the same remote key but a different file
                    // descriptor are conflicting.
                    conflicting.extend(self.outbound.iter().filter_map(|(c_fd, other)| {
                        if other.remote_id == remote_id && *c_fd != fd {
                            other.res_id.map(|c_id| (c_id, Direction::Outbound))
                        } else {
                            None
                        }
                    }));

                    for (c_id, c_link) in conflicting {
                        // If we have precedence, the inbound connection is closed.
                        // In the case where both connections are inbound or outbound,
                        // we close the newer connection, ie. the one with the higher
                        // resource id.
                        let close = match (direction, c_link) {
                            (Direction::Inbound, Direction::Outbound) => {
                                if precedence {
                                    res_id
                                } else {
                                    c_id
                                }
                            }
                            (Direction::Outbound, Direction::Inbound) => {
                                if precedence {
                                    c_id
                                } else {
                                    res_id
                                }
                            }
                            (Direction::Inbound, Direction::Inbound) => res_id.max(c_id),
                            (Direction::Outbound, Direction::Outbound) => res_id.max(c_id),
                        };

                        #[cfg(feature = "log")]
                        log::warn!(
                            target: "node-service", "Established session (id={res_id}) conflicts with existing session for {remote_id} (id={c_id})"
                        );
                        disconnect.push(close);
                    }
                }
                for conflicting_id in &disconnect {
                    #[cfg(feature = "log")]
                    log::warn!(
                        target: "node-service", "Closing conflicting session (id={conflicting_id}) with {remote_id}.."
                    );
                    // Disconnect and return the associated remote node ID of the remote, if
                    // available.
                    if let Some((remote_id, link)) =
                        self.disconnect(*conflicting_id, DisconnectReason::Conflict)
                    {
                        // We disconnect the session eagerly because otherwise we will get the new
                        // `connected` event before the `disconnect`, resulting in a duplicate
                        // connection.
                        self.controller.on_disconnected(
                            remote_id,
                            link,
                            &DisconnectReason::Conflict,
                        );
                    }
                }
                if !disconnect.contains(&res_id) {
                    self.remotes
                        .insert(res_id, Remote::connected(remote_id, addr.clone(), direction));
                    self.controller.on_established(remote_id, addr, direction, time);
                }
            }
            SessionEvent::Data(data) => {
                let Some(Remote::Connected {
                    remote_id,
                    marshaller,
                    ..
                }) = self.remotes.get_mut(&res_id)
                else {
                    #[cfg(feature = "log")]
                    log::warn!(target: "node-service", "Dropping message from unconnected remote (id={res_id})");
                    return;
                };

                let metrics = self.metrics.entry(*remote_id).or_default();
                metrics.bytes_received += data.len();

                if let Err(err) = marshaller.write_all(&data) {
                    #[cfg(feature = "log")]
                    log::error!(target: "node-service", "Unable to process messages fast enough for remote {res_id}; disconnecting");
                    self.disconnect(res_id, DisconnectReason::Framing(Arc::new(err)));

                    return;
                }

                loop {
                    match marshaller.pop::<C::Frame>() {
                        Ok(Some(frame)) => {
                            self.controller.on_frame(frame);
                        }
                        Ok(None) => {
                            // Buffer is empty, or frame isn't complete.
                            break;
                        }
                        Err(err) => {
                            #[cfg(feature = "log")]
                            log::error!(target: "node-service", "Invalid gossip message from {remote_id}: {err}");

                            if marshaller.read_queue_len() != 0 {
                                #[cfg(feature = "log")]
                                log::debug!(target: "node-service", "Dropping read buffer for {remote_id} with {} bytes", marshaller.read_queue_len());
                            }
                            self.controller.on_frame_unparsable(&err);
                            self.disconnect(res_id, DisconnectReason::Framing(Arc::new(err)));
                            break;
                        }
                    }
                }
            }
            SessionEvent::Terminated(err) => {
                self.disconnect(res_id, DisconnectReason::Connection(Arc::new(err)));
            }
        }
    }

    fn handle_registered(&mut self, fd: RawFd, id: ResourceId, ty: ResourceType) {
        match ty {
            ResourceType::Listener => {
                if let Some(local_addr) = self.listening.remove(&fd) {
                    self.controller.on_listening(local_addr);
                }
            }
            ResourceType::Transport => {
                if let Some(outbound) = self.outbound.get_mut(&fd) {
                    #[cfg(feature = "log")]
                    log::debug!(target: "node-service", "Outbound remote resource registered for {} with id={id} (fd={fd})", outbound.remote_id);
                    outbound.res_id = Some(id);
                } else if let Some(inbound) = self.inbound.get_mut(&fd) {
                    #[cfg(feature = "log")]
                    log::debug!(target: "node-service", "Inbound remote resource registered with id={id} (fd={fd})");
                    inbound.res_id = Some(id);
                } else {
                    #[cfg(feature = "log")]
                    log::warn!(target: "node-service", "Unknown remote registered with id={id} (fd={fd})");
                }
            }
        }
    }

    fn handle_command(&mut self, cmd: NodeCtl) {
        self.controller.on_command(cmd);
        if cmd == NodeCtl::Terminate {
            self.terminate();
        }
    }

    fn handle_error(&mut self, err: Error<Self::Listener, Self::Transport>) {
        match err {
            Error::Poll(err) => {
                // TODO: This should be a fatal error, there's nothing we can do here.
                #[cfg(feature = "log")]
                log::error!(target: "node-service", "Can't poll connections: {err}");
            }
            Error::ListenerDisconnect(id, _) => {
                // TODO: This should be a fatal error, there's nothing we can do here.
                #[cfg(feature = "log")]
                log::error!(target: "node-service", "Listener {id} disconnected");
            }
            Error::TransportDisconnect(id, transport) => {
                let fd = transport.as_raw_fd();
                #[cfg(feature = "log")]
                log::error!(target: "node-service", "Remote id={id} (fd={fd}) disconnected");

                // We're dropping the transport (and underlying network connection) here.
                drop(transport);

                // The remote transport is already disconnected and removed from the reactor;
                // therefore there is no need to initiate a disconnection. We simply remove
                // the remote from the map.
                match self.remotes.remove(&id) {
                    Some(remote) => {
                        if let Some(id) = remote.remote_id() {
                            self.controller.on_disconnected(
                                id,
                                remote.direction(),
                                &DisconnectReason::connection(),
                            );
                        } else {
                            #[cfg(feature = "log")]
                            log::debug!(target: "node-service", "Inbound disconnection before handshake; ignoring")
                        }
                    }
                    None => self.cleanup(id, fd),
                }
            }
        }
    }

    fn handover_listener(&mut self, res_id: ResourceId, listener: Self::Listener) {
        #[cfg(feature = "log")]
        log::debug!(target: "node-service", "Listener was unbound with id={res_id} (fd={})", listener.as_raw_fd());
        self.controller.on_unbound(listener)
    }

    fn handover_transport(&mut self, res_id: ResourceId, transport: Self::Transport) {
        let fd = transport.as_raw_fd();

        match self.remotes.entry(res_id) {
            Entry::Occupied(entry) => {
                match entry.get() {
                    Remote::Disconnecting {
                        remote_id,
                        reason,
                        direction,
                        ..
                    } => {
                        #[cfg(feature = "log")]
                        log::debug!(target: "node-service", "Transport handover for disconnecting remote with id={res_id} (fd={fd})");

                        // Disconnect TCP stream.
                        drop(transport);

                        // If there is no remote node ID, the service is not aware of the remote.
                        if let Some(remote_id) = remote_id {
                            // In the case of a conflicting connection, there will be two resources
                            // for the remote. However, at the service level, there is only one, and
                            // it is identified by remote node ID.
                            //
                            // Therefore, we specify which of the connections we're closing by
                            // passing the `link`.
                            self.controller.on_disconnected(*remote_id, *direction, reason);
                        }
                        entry.remove();
                    }
                    Remote::Connected { remote_id: id, .. } => {
                        panic!(
                            "Unexpected handover of connected remote {} with id={res_id} (fd={fd})",
                            id
                        );
                    }
                }
            }
            Entry::Vacant(_) => self.cleanup(res_id, fd),
        }
    }
}

impl<
        S: NetSession,
        L: NetListener<Stream = S::Connection>,
        C: NodeController<<S::Connection as NetConnection>::Addr, S, L>,
    > Iterator for NodeService<S, L, C>
{
    type Item = Action<NetAccept<S, L>, NetTransport<S>>;

    fn next(&mut self) -> Option<Self::Item> {
        // TODO: Remove once sender refactoring is complete
        self.actions.extend(self.controller.extract_actions());
        self.actions.pop_front()
    }
}

/// The client runtime containing reactor thread managing connection to the remote server and the
/// use of the server APIs.
pub struct Node {
    reactor: Reactor<NodeCtl, popol::Poller>, /* seems we do not need to pass any commands to
                                               * the
                                               * reactor */
}

impl Node {
    /// Constructs new client for client-server protocol. Takes service callback delegate and remote
    /// server address. Will attempt to connect to the server automatically once the reactor thread
    /// has started.
    pub fn new<
        S: NetSession + 'static,
        L: NetListener<Stream = S::Connection> + 'static,
        C: NodeController<<S::Connection as NetConnection>::Addr, S, L> + 'static,
    >(
        note_id: <S::Artifact as Artifact>::NodeId,
        delegate: C,
        listen: impl IntoIterator<Item = NetAccept<S, L>>,
    ) -> io::Result<Self> {
        let mut service = NodeService::<S, L, C>::new(note_id, delegate);
        for socket in listen {
            service.listen(socket);
        }
        let reactor = Reactor::named(service, popol::Poller::new(), s!("node-service"))?;
        Ok(Self { reactor })
    }

    /// Terminates the node, closing all connections, unbinding listeners and stopping the reactor
    /// thread.
    pub fn terminate(self) -> Result<(), Box<dyn Any + Send>> {
        self.reactor
            .controller()
            .cmd(NodeCtl::Terminate)
            .map_err(|err| Box::new(err) as Box<dyn Any + Send>)?;
        self.reactor.join()?;
        Ok(())
    }
}
