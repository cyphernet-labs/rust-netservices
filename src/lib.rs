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

#![cfg_attr(docsrs, feature(doc_auto_cfg))]

#[macro_use]
extern crate amplify;
#[cfg(feature = "log")]
extern crate log_crate as log;

pub mod frame;
#[cfg(feature = "reactor")]
pub mod tunnel;

mod connection;
mod listener;
pub mod session;
mod split;

#[cfg(feature = "io-reactor")]
pub mod resource;

pub use connection::{Address, NetConnection, NetStream};
pub use frame::{Frame, Marshaller};
pub use listener::NetListener;
#[cfg(feature = "io-reactor")]
pub use resource::{ListenerEvent, NetAccept, NetTransport, SessionEvent};
pub use session::{NetProtocol, NetSession, NetStateMachine};
pub use split::{NetReader, NetWriter, SplitIo, SplitIoError, TcpReader, TcpWriter};

#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub enum LinkDirection {
    Inbound,
    Outbound,
}

impl LinkDirection {
    pub fn is_inbound(self) -> bool { matches!(self, LinkDirection::Inbound) }
    pub fn is_outbound(self) -> bool { matches!(self, LinkDirection::Outbound) }
}
