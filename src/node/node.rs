// Library for building scalable privacy-preserving microservices P2P nodes
//
// SPDX-License-Identifier: Apache-2.0
//
// Written in 2022-2024 by
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

use std::collections::hash_map::Entry;
use std::collections::{HashMap, VecDeque};
use std::os::fd::{AsRawFd, RawFd};
use std::sync::Arc;
use std::{io, net};

use cyphernet::addr::Addr;
use reactor::poller::popol;
use reactor::{Action, Error, Reactor, Resource, ResourceId, ResourceType, Timestamp};

use super::{DisconnectReason, Inbound, Outbound, Remote, RemoteId, Remotes};
use crate::{
    Direction, ListenerEvent, NetAccept, NetConnection, NetListener, NetSession, NetTransport,
    SessionEvent,
};

#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub enum NodeCtl {}

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

pub trait NodeController<
    A: Addr + Send,
    I: RemoteId,
    S: NetSession,
    L: NetListener<Stream = S::Connection>,
>: Send
{
    type Request: TryFrom<Vec<u8>, Error: std::error::Error>;
    type Reply: Into<Vec<u8>>;

    fn extract_actions(
        &mut self,
    ) -> impl IntoIterator<Item = Action<NetAccept<S, L>, NetTransport<S>>>;

    fn should_accept(&mut self, remote: &A) -> bool;
    fn accept(&mut self, remote: A, connection: S::Connection)
        -> Result<S, impl std::error::Error>;

    fn on_listening(&mut self, socket: net::SocketAddr);

    fn on_disconnected(&mut self, id: I, direction: Direction, reason: &DisconnectReason);

    /// Called when a listener get disconnected and handed over by the reactor
    fn on_unbound(&mut self, listener: NetAccept<S, L>);

    fn on_tick(&mut self, time: Timestamp, metrics: &HashMap<I, Metrics>);

    fn on_command(&mut self, cmd: NodeCtl);

    fn on_timer(&mut self);

    fn on_request(&mut self, req: Self::Request, sender: impl Fn(Self::Reply));

    fn on_request_unparsable(
        &mut self,
        err: <Self::Request as TryFrom<Vec<u8>>>::Error,
        sender: impl Fn(Self::Reply),
    );
}

struct NodeService<
    I: RemoteId,
    S: NetSession,
    L: NetListener<Stream = S::Connection>,
    C: NodeController<<S::Connection as NetConnection>::Addr, I, S, L>,
> {
    /// Server message processor implementing server business logic.
    controller: C,
    listening: HashMap<RawFd, net::SocketAddr>,
    inbound: HashMap<RawFd, Inbound<<S::Connection as NetConnection>::Addr>>,
    outbound: HashMap<RawFd, Outbound<<S::Connection as NetConnection>::Addr, I>>,
    remotes: Remotes<<S::Connection as NetConnection>::Addr, I>,
    metrics: HashMap<I, Metrics>,
    /// Buffer for the actions which has to be delivered to the reactor when it calls the
    /// [`NodeService`] as an iterator.
    actions: VecDeque<Action<NetAccept<S, L>, NetTransport<S>>>,
}

impl<
        I: RemoteId,
        S: NetSession,
        L: NetListener<Stream = S::Connection>,
        C: NodeController<<S::Connection as NetConnection>::Addr, I, S, L>,
    > NodeService<I, S, L, C>
{
    pub fn new(controller: C) -> Self {
        NodeService {
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
    ) -> Option<(I, Direction)> {
        match self.remotes.entry(res_id) {
            Entry::Vacant(_) => {
                // Connecting remote with no session.
                #[cfg(feature = "log")]
                log::debug!(target: "wire", "Disconnecting pending remote with id={res_id}: {reason}");
                self.actions.push_back(Action::UnregisterTransport(res_id));

                // Check for attempted outbound connections. Unestablished inbound connections don't
                // have an ID yet.
                self.outbound
                    .values()
                    .find(|o| o.res_id == Some(res_id))
                    .map(|o| (o.id.clone(), Direction::Outbound))
            }
            Entry::Occupied(mut entry) => match entry.get() {
                Remote::Disconnecting { id, direction, .. } => {
                    #[cfg(feature = "log")]
                    log::error!(target: "wire", "Remote with id={res_id} is already disconnecting");

                    id.as_ref().map(|n| (n.clone(), *direction))
                }
                Remote::Connected { id, direction, .. } => {
                    #[cfg(feature = "log")]
                    log::debug!(target: "wire", "Disconnecting remote with id={res_id}: {reason}");

                    let direction = *direction;
                    let id = id.clone();

                    entry.insert(Remote::Disconnecting {
                        id: Some(id.clone()),
                        direction,
                        reason,
                    });
                    self.actions.push_back(Action::UnregisterTransport(res_id));

                    Some((id, direction))
                }
            },
        }
    }

    fn cleanup(&mut self, res_id: ResourceId, fd: RawFd) {
        if self.inbound.remove(&fd).is_some() {
            #[cfg(feature = "log")]
            log::debug!(target: "wire", "Cleaning up inbound peer state with id={res_id} (fd={fd})");
        } else if let Some(outbound) = self.outbound.remove(&fd) {
            #[cfg(feature = "log")]
            log::debug!(target: "wire", "Cleaning up outbound peer state with id={res_id} (fd={fd})");
            self.controller.on_disconnected(
                outbound.id,
                Direction::Outbound,
                &DisconnectReason::connection(),
            );
        } else {
            #[cfg(feature = "log")]
            log::warn!(target: "wire", "Tried to cleanup unknown peer with id={res_id} (fd={fd})");
        }
    }
}

impl<
        I: RemoteId,
        S: NetSession,
        L: NetListener<Stream = S::Connection>,
        C: NodeController<<S::Connection as NetConnection>::Addr, I, S, L>,
    > reactor::Handler for NodeService<I, S, L, C>
{
    type Listener = NetAccept<S, L>;
    type Transport = NetTransport<S>;
    type Command = NodeCtl;

    fn tick(&mut self, time: Timestamp) { self.controller.on_tick(time, &self.metrics) }

    fn handle_timer(&mut self) { self.controller.on_timer() }

    fn handle_listener_event(
        &mut self,
        _: ResourceId,
        event: <Self::Listener as Resource>::Event,
        _: Timestamp,
    ) {
        match event {
            ListenerEvent::Accepted(connection) => {
                let Ok(remote) = connection.remote_addr() else {
                    #[cfg(feature = "log")]
                    log::warn!(target: "wire", "Accepted connection doesn't have remote address; dropping");
                    drop(connection);

                    return;
                };
                let fd = connection.as_raw_fd();
                #[cfg(feature = "log")]
                log::debug!(target: "wire", "Inbound connection from {remote} (fd={fd})");

                // If the service doesn't want to accept this connection,
                // we drop the connection here, which disconnects the socket.
                if !self.controller.should_accept(&remote) {
                    #[cfg(feature = "log")]
                    log::debug!(target: "wire", "Rejecting inbound connection from {remote} (fd={fd})");
                    drop(connection);

                    return;
                }

                let session = match self.controller.accept(remote.clone(), connection) {
                    Ok(s) => s,
                    Err(e) => {
                        #[cfg(feature = "log")]
                        log::error!(target: "wire", "Error creating session for {remote}: {e}");
                        return;
                    }
                };
                let transport = match NetTransport::with_session(session, Direction::Inbound) {
                    Ok(transport) => transport,
                    Err(err) => {
                        #[cfg(feature = "log")]
                        log::error!(target: "wire", "Failed to create transport for accepted connection: {err}");
                        return;
                    }
                };
                #[cfg(feature = "log")]
                log::debug!(target: "wire", "Accepted inbound connection from {remote} (fd={fd})");

                self.inbound.insert(fd, Inbound {
                    res_id: None,
                    addr: remote.clone(),
                });
                self.actions.push_back(Action::RegisterTransport(transport))
            }
            ListenerEvent::Failure(err) => {
                #[cfg(feature = "log")]
                log::error!(target: "wire", "Error listening for inbound connections: {err}");
            }
        }
    }

    fn handle_transport_event(
        &mut self,
        id: ResourceId,
        event: <Self::Transport as Resource>::Event,
        time: Timestamp,
    ) {
        match event {
            SessionEvent::Established(_, _) => todo!(),
            SessionEvent::Data(_) => todo!(),
            SessionEvent::Terminated(err) => {
                self.disconnect(id, DisconnectReason::Connection(Arc::new(err)));
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
                    log::debug!(target: "wire", "Outbound remote resource registered for {} with id={id} (fd={fd})", outbound.id);
                    outbound.res_id = Some(id);
                } else if let Some(inbound) = self.inbound.get_mut(&fd) {
                    #[cfg(feature = "log")]
                    log::debug!(target: "wire", "Inbound remote resource registered with id={id} (fd={fd})");
                    inbound.res_id = Some(id);
                } else {
                    #[cfg(feature = "log")]
                    log::warn!(target: "wire", "Unknown remote registered with id={id} (fd={fd})");
                }
            }
        }
    }

    fn handle_command(&mut self, cmd: Self::Command) { self.controller.on_command(cmd) }

    fn handle_error(&mut self, err: Error<Self::Listener, Self::Transport>) {
        match err {
            Error::Poll(err) => {
                // TODO: This should be a fatal error, there's nothing we can do here.
                #[cfg(feature = "log")]
                log::error!(target: "wire", "Can't poll connections: {err}");
            }
            Error::ListenerDisconnect(id, _) => {
                // TODO: This should be a fatal error, there's nothing we can do here.
                #[cfg(feature = "log")]
                log::error!(target: "wire", "Listener {id} disconnected");
            }
            Error::TransportDisconnect(id, transport) => {
                let fd = transport.as_raw_fd();
                #[cfg(feature = "log")]
                log::error!(target: "wire", "Remote id={id} (fd={fd}) disconnected");

                // We're dropping the transport (and underlying network connection) here.
                drop(transport);

                // The remote transport is already disconnected and removed from the reactor;
                // therefore there is no need to initiate a disconnection. We simply remove
                // the remote from the map.
                match self.remotes.remove(&id) {
                    Some(mut remote) => {
                        if let Some(id) = remote.id() {
                            self.controller.on_disconnected(
                                id,
                                remote.direction(),
                                &DisconnectReason::connection(),
                            );
                        } else {
                            #[cfg(feature = "log")]
                            log::debug!(target: "wire", "Inbound disconnection before handshake; ignoring")
                        }
                    }
                    None => self.cleanup(id, fd),
                }
            }
        }
    }

    fn handover_listener(&mut self, res_id: ResourceId, listener: Self::Listener) {
        #[cfg(feature = "log")]
        log::debug!(target: "wire", "Listener was unbound with id={res_id} (fd={})", listener.as_raw_fd());
        self.controller.on_unbound(listener)
    }

    fn handover_transport(&mut self, res_id: ResourceId, transport: Self::Transport) {
        let fd = transport.as_raw_fd();

        match self.remotes.entry(res_id) {
            Entry::Occupied(entry) => {
                match entry.get() {
                    Remote::Disconnecting {
                        id,
                        reason,
                        direction,
                        ..
                    } => {
                        #[cfg(feature = "log")]
                        log::debug!(target: "wire", "Transport handover for disconnecting remote with id={res_id} (fd={fd})");

                        // Disconnect TCP stream.
                        drop(transport);

                        // If there is no NID, the service is not aware of the peer.
                        if let Some(id) = id {
                            // In the case of a conflicting connection, there will be two resources
                            // for the peer. However, at the service level, there is only one, and
                            // it is identified by NID.
                            //
                            // Therefore, we specify which of the connections we're closing by
                            // passing the `link`.
                            self.controller.on_disconnected(*id, *direction, reason);
                        }
                        entry.remove();
                    }
                    Remote::Connected { id, .. } => {
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
        I: RemoteId,
        S: NetSession,
        L: NetListener<Stream = S::Connection>,
        C: NodeController<<S::Connection as NetConnection>::Addr, I, S, L>,
    > Iterator for NodeService<I, S, L, C>
{
    type Item = Action<NetAccept<S, L>, NetTransport<S>>;

    fn next(&mut self) -> Option<Self::Item> {
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
        I: RemoteId + 'static,
        S: NetSession + 'static,
        L: NetListener<Stream = S::Connection> + 'static,
        C: NodeController<<S::Connection as NetConnection>::Addr, I, S, L> + 'static,
    >(
        delegate: C,
    ) -> io::Result<Self> {
        let service = NodeService::<I, S, L, C>::new(delegate);
        let reactor = Reactor::named(service, popol::Poller::new(), s!("node-reactor"))?;
        Ok(Self { reactor })
    }
}
