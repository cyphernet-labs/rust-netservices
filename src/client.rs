// Library for building scalable privacy-preserving microservices P2P nodes
//
// SPDX-License-Identifier: Apache-2.0
//
// Written in 2022-2023 by
//     Dr. Maxim Orlovsky <orlovsky@cyphernet.org>
//
// Copyright 2022-2023 Cyphernet DAO, Switzerland
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

//! A client for client-server communications.
//!
//! Supports both request-reply RPC and publish-subscribe, which operate in overlay mode.

use std::any::Any;
use std::collections::VecDeque;
use std::fmt::Debug;
use std::marker::PhantomData;
use std::os::fd::RawFd;
use std::time::Duration;
use std::{io, mem};

use reactor::poller::popol;
use reactor::{Action, Error, Reactor, ResourceId, ResourceType, Timestamp};

use crate::{Direction, ImpossibleResource, NetSession, NetTransport, SessionEvent};

/// The commands which are internally exchanged between [`Client`] runtime on the main thread and
/// [`ClientService`] existing inside the reactor thread.
///
/// When a user of the library calls [`Client`] method the actual command is passed to the
/// [`ClientCommand`] using this array.
///
/// Generic parameter `E` defines extra data used by more custom implementations of clients, like
/// RPC, PubSub etc.
#[derive(Debug)]
pub enum ClientCommand<E: Send + Debug = ()> {
    Send(Vec<u8>, E),
    Terminate,
}

#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub enum OnDisconnect {
    Terminate,
    Reconnect,
}

/// The set of callbacks used by the client to notify the business logic about events happening
/// inside the reactor.
pub trait ClientDelegate<A, S: NetSession, E: Send + Debug>: Send {
    /// The reply type which must be parsable from a byte blob.
    type Reply: TryFrom<Vec<u8>>;

    /// Asks the delegate to construct a connection to the remote server and return it as a form of
    /// [`NetSession`] to be registered and managed by the reactor and [`ClientService`] inside of
    /// it.
    fn connect(&self, remote: &A) -> S;

    /// Notifies about the successful establishment of the session with the server. The `attempt`
    /// argument specifies the number of the connection attempt which has succeeded, if a
    /// reconnection or failed connection had happened.
    fn on_established(&self, artifact: S::Artifact, attempt: usize);

    /// Notifies about failed connection to the server. As a response, the client business logic can
    /// ask to re-establish connection by returning [`OnDisconnect::Reconnect`]. Otherwise, the
    /// reactor will terminate.
    fn on_disconnect(&self, err: io::Error, attempt: usize) -> OnDisconnect;

    /// Callback for processing the message received from the server.
    fn on_reply(&mut self, reply: Self::Reply);

    /// Callback for processing invalid message received from the server which can't be parsed into
    /// [`Self::Reply`] type.
    // TODO: Make it just an error
    fn on_reply_unparsable(&self, err: <Self::Reply as TryFrom<Vec<u8>>>::Error);

    /// Callback for processing reactor [`Error`]s.
    fn on_error(&self, err: Error<ImpossibleResource, NetTransport<S>>);

    fn before_send(&mut self, data: Vec<u8>, _extra: E) -> Vec<u8> { data }
}

pub struct ClientService<A: Send, S: NetSession, D: ClientDelegate<A, S, E>, E: Send + Debug> {
    delegate: D,
    remote: A,
    connection_id: Option<ResourceId>,
    connection_fd: Option<RawFd>,
    attempting: bool,
    attempts: usize,
    data_stack: Vec<Vec<u8>>,
    action_queue: VecDeque<Action<ImpossibleResource, NetTransport<S>>>,
    _phantom: PhantomData<E>,
}

impl<A: Send, S: NetSession, D: ClientDelegate<A, S, E>, E: Send + Debug>
    ClientService<A, S, D, E>
{
    #[inline]
    pub fn new(delegate: D, remote: A) -> Self {
        Self {
            delegate,
            remote,
            connection_id: None,
            connection_fd: None,
            attempts: 0,
            attempting: false,
            data_stack: empty!(),
            action_queue: VecDeque::from(vec![Action::SetTimer(Duration::from_millis(0))]),
            _phantom: PhantomData,
        }
    }

    fn connect(&mut self) {
        loop {
            let session = self.delegate.connect(&self.remote);
            match NetTransport::with_session(session, Direction::Outbound) {
                Ok(transport) => {
                    self.action_queue.push_back(Action::RegisterTransport(transport));
                    break;
                }
                Err(err) => {
                    self.attempts += 1;
                    if self.delegate.on_disconnect(err, self.attempts) == OnDisconnect::Terminate {
                        self.terminate();
                        break;
                    }
                }
            }
        }
    }

    fn terminate(&mut self) { self.action_queue.push_back(Action::Terminate); }
}

impl<A: Send, S: NetSession, D: ClientDelegate<A, S, E>, E: Send + Debug> reactor::Handler
    for ClientService<A, S, D, E>
{
    type Listener = ImpossibleResource;
    type Transport = NetTransport<S>;
    type Command = ClientCommand<E>;

    fn tick(&mut self, time: Timestamp) {
        #[cfg(feature = "log")]
        log::trace!(target: "netservices-client", "reactor tick at {time}");
    }

    fn handle_timer(&mut self) {
        #[cfg(feature = "log")]
        log::trace!(target: "netservices-client", "reactor timer event");
        if !self.attempting {
            self.attempting = true;
            self.connect();
        }
    }

    fn handle_listener_event(&mut self, _: ResourceId, _: (), _: Timestamp) {
        unreachable!("there is no listener in client")
    }

    fn handle_transport_event(&mut self, id: ResourceId, event: SessionEvent<S>, time: Timestamp) {
        match event {
            SessionEvent::Established(fd, artifact) => {
                #[cfg(feature = "log")]
                log::debug!(target: "netservices-client", "established connection to server (fd={fd})");
                self.connection_fd = Some(fd);
                self.delegate.on_established(artifact, self.attempts);
            }
            SessionEvent::Data(data) => match D::Reply::try_from(data) {
                Ok(reply) => self.delegate.on_reply(reply),
                Err(err) => self.delegate.on_reply_unparsable(err),
            },
            SessionEvent::Terminated(err) => {
                self.connection_id = None;
                self.connection_fd = None;
                self.attempts += 1;
                #[cfg(feature = "log")]
                log::debug!(target: "netservices-client", "disconnected from the server for the {} time", self.attempts);
                if self.delegate.on_disconnect(err, self.attempts) == OnDisconnect::Reconnect {
                    self.connect();
                } else {
                    self.terminate();
                }
            }
        }
    }

    fn handle_registered(&mut self, fd: RawFd, id: ResourceId, ty: ResourceType) {
        #[cfg(feature = "log")]
        log::trace!(target: "netservices-client", "handled registration of connection with fd={fd}, id={id} on attempt {}", self.attempts);
        debug_assert_eq!(ty, ResourceType::Transport);
        debug_assert_eq!(self.connection_fd, Some(fd));
        self.connection_id = Some(id);
        let mut data_stack = vec![];
        mem::swap(&mut data_stack, &mut self.data_stack);
        self.action_queue.extend(data_stack.into_iter().map(|data| Action::Send(id, data)));
    }

    fn handle_command(&mut self, cmd: Self::Command) {
        match cmd {
            ClientCommand::Send(data, extra) => {
                let data = self.delegate.before_send(data, extra);
                if let Some(id) = self.connection_id {
                    self.action_queue.push_back(Action::Send(id, data));
                } else {
                    self.data_stack.push(data);
                }
            }
            ClientCommand::Terminate => {
                self.terminate();
            }
        }
    }

    fn handle_error(&mut self, err: Error<Self::Listener, Self::Transport>) {
        self.delegate.on_error(err)
    }

    fn handover_listener(&mut self, _: ResourceId, _: Self::Listener) {
        unreachable!("there is no listener in client")
    }

    fn handover_transport(&mut self, id: ResourceId, transport: Self::Transport) {
        #[cfg(feature = "log")]
        log::trace!(target: "netservices-client", "transport {} has been disconnected and handover (id={id})", transport.display());
        self.connection_id = None;
        self.connection_fd = None;
        // TODO: Check that we do not need to do anything else
    }
}

impl<A: Send, S: NetSession, D: ClientDelegate<A, S, E>, E: Send + Debug> Iterator
    for ClientService<A, S, D, E>
{
    type Item = Action<ImpossibleResource, NetTransport<S>>;

    fn next(&mut self) -> Option<Self::Item> { self.action_queue.pop_front() }
}

/// The client runtime containing reactor thread managing connection to the remote server and the
/// use of the server APIs.
pub struct Client<E: Send + Debug = ()> {
    reactor: Reactor<ClientCommand<E>, popol::Poller>,
}

impl<E> Client<E>
where E: Send + Debug + 'static
{
    pub fn new<A: Send + 'static, S: NetSession + 'static, D: ClientDelegate<A, S, E> + 'static>(
        delegate: D,
        remote: A,
    ) -> io::Result<Self> {
        let service = ClientService::<A, S, D, E>::new(delegate, remote);
        let reactor = Reactor::named(service, popol::Poller::new(), s!("client"))?;
        Ok(Self { reactor })
    }

    pub fn terminate(self) -> Result<(), Box<dyn Any + Send>> {
        self.reactor
            .controller()
            .cmd(ClientCommand::Terminate)
            .map_err(|err| Box::new(err) as Box<dyn Any + Send>)?;
        self.reactor.join()?;
        Ok(())
    }
}

impl Client {
    pub fn send(&self, data: impl Into<Vec<u8>>) -> io::Result<()> {
        self.reactor.controller().cmd(ClientCommand::Send(data.into(), ()))
    }
}
