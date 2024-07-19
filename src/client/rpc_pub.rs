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

use std::any::Any;
use std::collections::HashMap;
use std::io;
use std::io::Error;
use std::marker::PhantomData;

use super::{Client, ClientDelegate, OnDisconnect};
use crate::{ImpossibleResource, NetSession, NetTransport};

pub type Cb = fn(Vec<u8>);

pub const CLIENT_MSG_ID_RPC: u8 = 0x01u8;
pub const CLIENT_MSG_ID_PUB: u8 = 0x10u8;

#[derive(Copy, Clone, Eq, PartialEq, Hash, Debug)]
pub enum RpcPubId {
    Rpc(u64),
    Pub(u16),
}

#[derive(Clone, Eq, PartialEq, Hash, Debug)]
pub struct Msg {
    pub id: RpcPubId,
    pub payload: Vec<u8>,
}

#[derive(Clone, Eq, PartialEq, Hash, Debug, Display, Error)]
#[display(doc_comments)]
pub enum ParseMsgErr {
    /// the received message is empty and lacks RPCPub identifier.
    Empty,

    /// the received message is too short and doesn't provide RPCPub identifier.
    NoId,

    /// the received message contains invalid id tag ({_0:#04x}).
    InvalidTag(u8),
}

impl TryFrom<Vec<u8>> for Msg {
    type Error = ParseMsgErr;

    fn try_from(data: Vec<u8>) -> Result<Self, Self::Error> {
        let Some(tag) = data.get(0) else {
            return Err(ParseMsgErr::Empty);
        };
        let mut data = &data[1..];
        let id = match *tag {
            CLIENT_MSG_ID_RPC if data.len() < 8 => return Err(ParseMsgErr::NoId),
            CLIENT_MSG_ID_PUB if data.len() < 2 => return Err(ParseMsgErr::NoId),
            CLIENT_MSG_ID_RPC => {
                let mut id = [0u8; 8];
                id.copy_from_slice(&data[0..8]);
                data = &data[8..];
                RpcPubId::Rpc(u64::from_le_bytes(id))
            }
            CLIENT_MSG_ID_PUB => {
                let mut id = [0u8; 2];
                id.copy_from_slice(&data[0..2]);
                data = &data[2..];
                RpcPubId::Pub(u16::from_le_bytes(id))
            }
            wrong => return Err(ParseMsgErr::InvalidTag(wrong)),
        };
        let payload = data.to_vec();
        Ok(Msg { id, payload })
    }
}

impl From<Msg> for Vec<u8> {
    fn from(msg: Msg) -> Self {
        let mut data = Vec::with_capacity(msg.payload.len() + 9);
        match msg.id {
            RpcPubId::Rpc(id) => {
                data.push(CLIENT_MSG_ID_RPC);
                data.extend(id.to_le_bytes());
            }
            RpcPubId::Pub(id) => {
                data.push(CLIENT_MSG_ID_PUB);
                data.extend(id.to_le_bytes());
            }
        }
        data.extend(msg.payload);
        data
    }
}

pub trait RpcPubDelegate<A: Send, S: NetSession>: ClientDelegate<A, S, Cb, Reply = Msg> {
    fn on_message_pub(&self, id: u16, data: Vec<u8>);
    // TODO: Make it just an error
    fn on_reply_unrecognized(&self, id: u64, data: Vec<u8>);
    fn on_reply_timeout(&self, id: u64);
}

pub struct RpcPubService<A: Send, S: NetSession, D: RpcPubDelegate<A, S>> {
    delegate: D,
    last_id: u64,
    callbacks: HashMap<u64, Cb>,
    _phantom: PhantomData<(A, S)>,
}

impl<A: Send, S: NetSession, D: RpcPubDelegate<A, S>> RpcPubService<A, S, D> {
    pub fn new(delegate: D) -> Self {
        RpcPubService {
            delegate,
            last_id: 0,
            callbacks: empty!(),
            _phantom: PhantomData,
        }
    }
}

impl<A: Send, S: NetSession, D: RpcPubDelegate<A, S>> ClientDelegate<A, S, Cb>
    for RpcPubService<A, S, D>
{
    type Reply = Msg;

    fn connect(&self, remote: &A) -> S { self.delegate.connect(remote) }

    fn on_established(&self, artifact: S::Artifact, attempt: usize) {
        self.delegate.on_established(artifact, attempt)
    }

    fn on_disconnect(&self, err: Error, attempt: usize) -> OnDisconnect {
        self.delegate.on_disconnect(err, attempt)
    }

    fn on_reply(&mut self, msg: Msg) {
        match msg.id {
            RpcPubId::Rpc(id) => {
                if let Some(cb) = self.callbacks.remove(&id) {
                    cb(msg.payload)
                } else {
                    self.delegate.on_reply_unrecognized(id, msg.payload);
                }
            }
            RpcPubId::Pub(id) => self.delegate.on_message_pub(id, msg.payload),
        }
    }

    fn on_reply_unparsable(&self, err: ParseMsgErr) { self.delegate.on_reply_unparsable(err) }

    fn on_error(&self, err: reactor::Error<ImpossibleResource, NetTransport<S>>) {
        self.delegate.on_error(err)
    }

    fn before_send(&mut self, data: Vec<u8>, cb: Cb) -> Vec<u8> {
        let id = self.last_id;
        let mut req = Vec::with_capacity(data.len() + 9);
        let check = self.callbacks.insert(id, cb);
        debug_assert!(check.is_none());
        req.push(CLIENT_MSG_ID_RPC);
        req.extend(id.to_le_bytes());
        req.extend(data);
        self.last_id += 1;
        req
    }
}

/// The client runtime containing reactor thread managing connection to the remote server and the
/// use of the server APIs.
pub struct RpcPubClient {
    inner: Client<Cb>,
}

impl RpcPubClient {
    pub fn new<A: Send + 'static, S: NetSession + 'static, D: RpcPubDelegate<A, S> + 'static>(
        delegate: D,
        remote: A,
    ) -> io::Result<Self> {
        let rpc_pub_service = RpcPubService::new(delegate);
        let client = Client::new(rpc_pub_service, remote)?;
        Ok(Self { inner: client })
    }

    pub fn send(&mut self, data: impl Into<Vec<u8>>, cb: Cb) -> io::Result<()> {
        self.inner.send_extra(data, cb)
    }

    pub fn terminate(self) -> Result<(), Box<dyn Any + Send>> { self.inner.terminate() }
}
