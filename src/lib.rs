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

#![cfg_attr(docsrs, feature(doc_auto_cfg))]

#[macro_use]
extern crate amplify;
#[cfg(feature = "log")]
extern crate log_crate as log;

pub mod frame;

mod connection;
mod listener;
pub mod session;
mod split;

#[cfg(feature = "reactor")]
pub mod resource;
#[cfg(feature = "reactor")]
pub mod client;
#[cfg(feature = "reactor")]
pub mod server;
#[cfg(feature = "reactor")]
pub mod node;

pub const READ_BUFFER_SIZE: usize = u16::MAX as usize;

pub use connection::{Address, AsConnection, NetConnection, NetStream};
pub use frame::{Frame, Marshaller};
pub use listener::NetListener;
#[cfg(feature = "reactor")]
pub use resource::{ImpossibleResource, ListenerEvent, NetAccept, NetTransport, SessionEvent};
#[cfg(feature = "reactor")]
pub use server::tunnel;
pub use session::{Artifact, NetProtocol, NetSession, NetStateMachine, NodeId};
pub use split::{NetReader, NetWriter, SplitIo, SplitIoError, TcpReader, TcpWriter};

#[derive(Copy, Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Debug, Display)]
#[display("lowercase")]
pub enum Direction {
    Inbound,
    Outbound,
}

impl Direction {
    pub fn is_inbound(self) -> bool { matches!(self, Direction::Inbound) }
    pub fn is_outbound(self) -> bool { matches!(self, Direction::Outbound) }
}
