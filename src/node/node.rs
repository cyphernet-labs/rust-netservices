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
use std::{io, net};

use cyphernet::addr::Addr;
use cyphernet::EcPk;
use reactor::poller::popol;
use reactor::{Action, Error, Reactor, Resource, ResourceId, ResourceType, Timestamp};

use super::{DisconnectReason, Inbound, Outbound, Remote, Remotes};
use crate::{
    Direction, ListenerEvent, NetAccept, NetConnection, NetListener, NetSession, NetTransport,
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
    I: EcPk,
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

    fn on_tick(&mut self, time: Timestamp, metrics: &HashMap<I, Metrics>);

    fn on_timer(&mut self);

    fn on_request(&mut self, req: Self::Request, sender: impl Fn(Self::Reply));

    fn on_request_unparsable(
        &mut self,
        err: <Self::Request as TryFrom<Vec<u8>>>::Error,
        sender: impl Fn(Self::Reply),
    );
}

struct NodeService<
    I: EcPk,
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
        I: EcPk,
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
}

impl<
        I: EcPk,
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
                    log::warn!(target: "wire", "Accepted connection doesn't have remote address; dropping..");
                    drop(connection);

                    return;
                };
                let fd = connection.as_raw_fd();
                #[cfg(feature = "log")]
                log::debug!(target: "wire", "Inbound connection from {remote} (fd={fd})..");

                // If the service doesn't want to accept this connection,
                // we drop the connection here, which disconnects the socket.
                if !self.controller.should_accept(&remote) {
                    #[cfg(feature = "log")]
                    log::debug!(target: "wire", "Rejecting inbound connection from {remote} (fd={fd})..");
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
                log::debug!(target: "wire", "Accepted inbound connection from {remote} (fd={fd})..");

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
        todo!()
    }

    fn handle_registered(&mut self, fd: RawFd, id: ResourceId, ty: ResourceType) { todo!() }

    fn handle_command(&mut self, cmd: Self::Command) { todo!() }

    fn handle_error(&mut self, err: Error<Self::Listener, Self::Transport>) { todo!() }

    fn handover_listener(&mut self, id: ResourceId, listener: Self::Listener) { todo!() }

    fn handover_transport(&mut self, id: ResourceId, transport: Self::Transport) { todo!() }
}

impl<
        I: EcPk,
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
        I: EcPk + 'static,
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
