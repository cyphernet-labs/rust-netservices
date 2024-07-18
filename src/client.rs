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

use std::collections::VecDeque;
use std::fmt::Debug;
use std::io;
use std::os::fd::RawFd;
use std::time::Duration;

use reactor::poller::popol;
use reactor::{Action, Error, Reactor, ResourceId, ResourceType, Timestamp};

use crate::{Direction, ImpossibleResource, NetSession, NetTransport, SessionEvent};

#[derive(Debug)]
pub enum ClientCommand {}

#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub enum OnDisconnect {
    Terminate,
    Reconnect,
}

pub trait ClientDelegate<C, S: NetSession>: Send {
    type Reply: TryFrom<Vec<u8>>;
    fn connect(&self, remote: &C) -> S;
    fn established(&self, artifact: S::Artifact, attempt: usize);
    fn disconnected(&self, err: io::Error, attempt: usize) -> OnDisconnect;
    fn process(&self, reply: Self::Reply);
    fn invalid_message(&self, data: <Self::Reply as TryFrom<Vec<u8>>>::Error);
}

pub struct ClientService<C: Send, S: NetSession, D: ClientDelegate<C, S>> {
    delegate: D,
    remote: C,
    connection: Option<RawFd>,
    attempting: bool,
    attempts: usize,
    action_queue: VecDeque<Action<ImpossibleResource, NetTransport<S>>>,
}

impl<C: Send, S: NetSession, D: ClientDelegate<C, S>> ClientService<C, S, D> {
    #[inline]
    pub fn new(delegate: D, remote: C) -> Self {
        Self {
            delegate,
            remote,
            connection: None,
            attempts: 0,
            attempting: false,
            action_queue: VecDeque::from(vec![Action::SetTimer(Duration::from_millis(0))]),
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
                    if self.delegate.disconnected(err, self.attempts) == OnDisconnect::Terminate {
                        self.terminate();
                        break;
                    }
                }
            }
        }
    }

    fn terminate(&self) { todo!() }
}

impl<C: Send, S: NetSession, D: ClientDelegate<C, S>> reactor::Handler for ClientService<C, S, D> {
    type Listener = ImpossibleResource;
    type Transport = NetTransport<S>;
    type Command = ClientCommand;

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
                log::debug!(target: "netservices-client", "established connection to server (fd={fd})");
                self.connection = Some(fd);
                self.delegate.established(artifact, self.attempts);
            }
            SessionEvent::Data(data) => match D::Reply::try_from(data) {
                Ok(reply) => self.delegate.process(reply),
                Err(err) => self.delegate.invalid_message(err),
            },
            SessionEvent::Terminated(err) => {
                self.attempts += 1;
                log::debug!(target: "netservices-client", "disconnected from the server for the {} time", self.attempts);
                if self.delegate.disconnected(err, self.attempts) == OnDisconnect::Reconnect {
                    self.connect();
                } else {
                    self.terminate();
                }
            }
        }
    }

    fn handle_registered(&mut self, fd: RawFd, id: ResourceId, _: ResourceType) {
        log::trace!(target: "netservices-client", "handled registration of connection with fd={fd}, id={id} on attempt {}", self.attempts);
    }

    fn handle_command(&mut self, cmd: Self::Command) { match cmd {} }

    fn handle_error(&mut self, err: Error<Self::Listener, Self::Transport>) { todo!() }

    fn handover_listener(&mut self, _: ResourceId, _: Self::Listener) {
        unreachable!("there is no listener in client")
    }

    fn handover_transport(&mut self, id: ResourceId, transport: Self::Transport) { todo!() }
}

impl<C: Send, S: NetSession, D: ClientDelegate<C, S>> Iterator for ClientService<C, S, D> {
    type Item = Action<ImpossibleResource, NetTransport<S>>;

    fn next(&mut self) -> Option<Self::Item> { self.action_queue.pop_front() }
}

pub struct Client {
    reactor: Reactor<ClientCommand, popol::Poller>,
}

impl Client {
    pub fn new<C: Send + 'static, S: NetSession + 'static, D: ClientDelegate<C, S> + 'static>(
        delegate: D,
        remote: C,
    ) -> io::Result<Self> {
        let service = ClientService::<C, S, D>::new(delegate, remote);
        let reactor = Reactor::named(service, popol::Poller::new(), s!("client"))?;
        Ok(Self { reactor })
    }
}
