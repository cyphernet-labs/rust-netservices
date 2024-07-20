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

use std::collections::VecDeque;
use std::io;
use std::marker::PhantomData;
use std::os::fd::RawFd;

use reactor::poller::popol;
use reactor::{Action, Error, Reactor, Resource, ResourceId, ResourceType, Timestamp};

use crate::client::ConnectionDelegate;
use crate::{NetAccept, NetListener, NetSession, NetTransport};

pub trait ServerDelegate<A, S: NetSession>: ConnectionDelegate<A, S> {
    type Request: TryFrom<Vec<u8>, Error: std::error::Error>;
    type Reply: Into<Vec<u8>>;

    fn on_request(&mut self, req: Self::Request, sender: impl Fn(Self::Reply));

    fn on_request_unparsable(
        &mut self,
        err: <Self::Request as TryFrom<Vec<u8>>>::Error,
        sender: impl Fn(Self::Reply),
    );
}

struct ServerService<
    A: Send,
    S: NetSession,
    L: NetListener<Stream = S::Connection>,
    D: ServerDelegate<A, S>,
> {
    /// Connection and messaging delegate implementing server business logic.
    delegate: D,
    /// Buffer for the actions which has to be delivered to the reactor when it calls the
    /// [`ClientServer`] as an iterator.
    action_queue: VecDeque<Action<NetAccept<S, L>, NetTransport<S>>>,
    _phantom: PhantomData<(A, S, L)>,
}

impl<A: Send, S: NetSession, L: NetListener<Stream = S::Connection>, D: ServerDelegate<A, S>>
    ServerService<A, S, L, D>
{
    pub fn new(delegate: D) -> Self {
        ServerService {
            delegate,
            action_queue: empty!(),
            _phantom: PhantomData,
        }
    }
}

impl<A: Send, S: NetSession, L: NetListener<Stream = S::Connection>, D: ServerDelegate<A, S>>
    reactor::Handler for ServerService<A, S, L, D>
{
    type Listener = NetAccept<S, L>;
    type Transport = NetTransport<S>;
    type Command = ();

    fn tick(&mut self, time: Timestamp) { todo!() }

    fn handle_timer(&mut self) { todo!() }

    fn handle_listener_event(
        &mut self,
        id: ResourceId,
        event: <Self::Listener as Resource>::Event,
        time: Timestamp,
    ) {
        todo!()
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

impl<A: Send, S: NetSession, L: NetListener<Stream = S::Connection>, D: ServerDelegate<A, S>>
    Iterator for ServerService<A, S, L, D>
{
    type Item = Action<NetAccept<S, L>, NetTransport<S>>;

    fn next(&mut self) -> Option<Self::Item> { self.action_queue.pop_front() }
}

/// The client runtime containing reactor thread managing connection to the remote server and the
/// use of the server APIs.
pub struct Server {
    reactor: Reactor<(), popol::Poller>, /* seems we do not need to pass any commands to the
                                          * reactor */
}

impl Server {
    /// Constructs new client for client-server protocol. Takes service callback delegate and remote
    /// server address. Will attempt to connect to the server automatically once the reactor thread
    /// has started.
    pub fn new<
        A: Send + 'static,
        S: NetSession + 'static,
        L: NetListener<Stream = S::Connection> + 'static,
        D: ServerDelegate<A, S> + 'static,
    >(
        delegate: D,
    ) -> io::Result<Self> {
        let service = ServerService::<A, S, L, D>::new(delegate);
        let reactor = Reactor::named(service, popol::Poller::new(), s!("server"))?;
        Ok(Self { reactor })
    }
}

impl Server {}
