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

use std::collections::{hash_map, HashMap};
use std::fmt;
use std::fmt::Debug;
use std::sync::Arc;

use cyphernet::addr::Addr;
use cyphernet::EcPk;
use reactor::ResourceId;

use crate::{Direction, Marshaller};

/// Disconnect reason.
#[derive(Clone, Debug, Display)]
#[display(inner)]
pub enum DisconnectReason {
    /// Error while dialing the remote. This error occurs before a connection is
    /// even established. Errors of this kind are usually not transient.
    Dial(Arc<dyn std::error::Error + Sync + Send>),
    /// Error with an underlying established connection. Sometimes, reconnecting
    /// after such an error is possible.
    Connection(Arc<dyn std::error::Error + Sync + Send>),
    /// Session error.
    Session(Arc<dyn std::error::Error + Sync + Send>),
    /// Session conflicts with existing session.
    #[display("conflict")]
    Conflict,
    /// Connection to self.
    #[display("self-connection")]
    SelfConnection,
    /// User requested disconnect.
    #[display("command")]
    Command,
}

/// The initial state of an outbound peer before handshake is completed.
/// The initial state of an inbound remote before handshake is completed.
#[derive(Debug)]
pub struct Outbound<A: Addr, I: EcPk> {
    /// Resource ID, if registered.
    pub res_id: Option<ResourceId>,
    /// Remote address.
    pub addr: A,
    /// Remote identity
    pub id: I,
}

/// The initial state of an inbound remote before handshake is completed.
#[derive(Debug)]
pub struct Inbound<A: Addr> {
    /// Resource ID, if registered.
    pub res_id: Option<ResourceId>,
    /// Remote address.
    pub addr: A,
}

#[derive(Clone)]
pub enum Remote<A: Addr, I: EcPk> {
    /// The state after handshake is completed. Remotes in this state are handled by the underlying
    /// processor.
    Connected {
        addr: A,
        id: I,
        inbox: Marshaller,
        direction: Direction,
    },
    /// The peer was scheduled for disconnection. Once the transport is handed over
    /// by the reactor, we can consider it disconnected.
    Disconnecting {
        id: Option<I>,
        reason: DisconnectReason,
        direction: Direction,
    },
}

impl<A: Addr + Debug, I: EcPk + Debug> Debug for Remote<A, I> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Connected {
                addr,
                id,
                direction,
                ..
            } => write!(f, "Connected({direction}, {addr:?}, {id:?})"),
            Self::Disconnecting { .. } => write!(f, "Disconnecting"),
        }
    }
}

impl<A: Addr, I: EcPk> Remote<A, I> {
    /// Return the remote id, if any.
    pub fn id(&self) -> Option<&I> {
        match self {
            Self::Connected { id, .. } | Self::Disconnecting { id: Some(id), .. } => Some(id),
            Self::Disconnecting { id: None, .. } => None,
        }
    }

    pub fn addr(&self) -> Option<&A> {
        match self {
            Remote::Connected { addr, .. } => Some(addr),
            Remote::Disconnecting { .. } => None,
        }
    }

    pub fn direction(&self) -> Direction {
        match self {
            Self::Connected { direction, .. } => *direction,
            Self::Disconnecting { direction, .. } => *direction,
        }
    }

    /// Connected remote.
    pub fn connected(id: I, addr: A, direction: Direction) -> Self {
        Self::Connected {
            direction,
            addr,
            id,
            inbox: Marshaller::default(),
        }
    }
}

/// Holds connected remotes.
#[derive(Clone, Debug)]
pub struct Remotes<A: Addr, I: EcPk>(HashMap<ResourceId, Remote<A, I>>);

impl<A: Addr, I: EcPk> Default for Remotes<A, I> {
    fn default() -> Self { Self(empty!()) }
}

impl<A: Addr, I: EcPk> Remotes<A, I> {
    pub fn get_mut(&mut self, res_id: &ResourceId) -> Option<&mut Remote<A, I>> {
        self.0.get_mut(res_id)
    }

    pub fn entry(&mut self, res_id: ResourceId) -> hash_map::Entry<ResourceId, Remote<A, I>> {
        self.0.entry(res_id)
    }

    pub fn insert(&mut self, res_id: ResourceId, peer: Remote<A, I>) {
        if self.0.insert(res_id, peer).is_some() {
            #[cfg(feature = "log")]
            log::warn!(target: "node-service", "Replacing existing remote with resource id {res_id}");
        }
    }

    pub fn remove(&mut self, res_id: &ResourceId) -> Option<Remote<A, I>> { self.0.remove(res_id) }

    pub fn lookup(&self, id: &I) -> Option<(ResourceId, &Remote<A, I>)> {
        self.0
            .iter()
            .find(|(_, remote)| remote.id() == Some(id))
            .map(|(res_id, remote)| (*res_id, remote))
    }

    pub fn lookup_mut(&mut self, id: &I) -> Option<(ResourceId, &mut Remote<A, I>)> {
        self.0
            .iter_mut()
            .find(|(_, remote)| remote.id() == Some(id))
            .map(|(res_id, remote)| (*res_id, remote))
    }

    pub fn active(&self) -> impl Iterator<Item = (ResourceId, &I, Direction)> {
        self.0.iter().filter_map(|(res_id, remote)| match remote {
            Remote::Connected { id, direction, .. } => Some((*res_id, id, *direction)),
            Remote::Disconnecting { .. } => None,
        })
    }
}
