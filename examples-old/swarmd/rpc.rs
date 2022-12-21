use std::collections::BTreeSet;
use std::io::Write;

use netservices::nxk_tcp::NxkAddr;
use netservices::peer;
use re_actor::actors::IoEv;
use re_actor::{Actor, Controller, InternalError, ReactorApi};
use streampipes::Stream;

use crate::daemon::{ActorId, Message, Threads};
use crate::persistence::PersistenceCmd;
use crate::router::RouterCmd;
use crate::ResourceId;
use crate::{daemon, PeerId, RouteMap};

pub type ClientId = u64;

#[derive(Debug)]
pub enum Request {
    ListPeers,
    ConnectPeer(NxkAddr),
    DisconnectPeer(PeerId),

    LocalResources,
    AllResources,
    PublishResource(ResourceId, Vec<u8>),
    RetrieveResource(ResourceId),
}

#[derive(Debug)]
pub enum Response {
    Success,
    Failure(String),
    Peers(BTreeSet<PeerId>),
    LocalResources(BTreeSet<ResourceId>),
    KnownResources(RouteMap),
}

pub struct RpcActor {
    id: ClientId,
    stream: Box<dyn Stream>,
    controller: Controller<Threads>,
}

impl Actor for RpcActor {
    type Layout = Threads;
    type Id = ClientId;
    type Context = (ClientId, Box<dyn Stream + Send>);
    type Cmd = Response;
    type Error = daemon::Error;

    fn with(
        context: Self::Context,
        controller: Controller<Self::Layout>,
    ) -> Result<Self, Self::Error>
    where
        Self: Sized,
    {
        Ok(Self {
            id: context.0,
            stream: context.1,
            controller,
        })
    }

    fn id(&self) -> Self::Id {
        self.id
    }

    fn io_ready(&mut self, io: IoEv) -> Result<(), Self::Error> {
        if io.is_readable {
            // TODO: deserialize RPC request from the stream
            for request in &self.stream {
                self.handle_request(request)?;
            }
        }
        if io.is_writable {
            self.stream.flush().map_err(InternalError::from)?;
        }
        Ok(())
    }

    fn handle_cmd(&mut self, cmd: Self::Cmd) -> Result<(), Self::Error> {
        self.stream.write_all(cmd)
    }

    fn handle_err(&mut self, err: Self::Error) -> Result<(), Self::Error> {
        log::error!("RPC actor error: {}", err);
        self.stream.write_all(Response::Failure(err.to_string()))
    }
}

impl RpcActor {
    fn handle_request(&mut self, request: Request) -> Result<(), InternalError<Threads>> {
        let id = ActorId::Rpc(self.id);
        match request {
            Request::ListPeers => self
                .controller
                .send(ActorId::Router, Message::Router(RouterCmd::ListPeers(id))),

            Request::ConnectPeer(addr) => {
                let ctx = daemon::Context::R2p(peer::Context {
                    method: peer::Action::Connect(addr),
                    local_node: self.local_node,
                });
                self.controller.start_actor(Threads::P2p, ctx)
            }

            Request::DisconnectPeer(id) => self.controller.stop_actor(ActorId::P2p(id)),

            Request::LocalResources => self.controller.send(
                ActorId::Router,
                Message::Router(RouterCmd::ListLocalResources(id)),
            ),

            Request::AllResources => self.controller.send(
                ActorId::Router,
                Message::Router(RouterCmd::ListAllResources(id)),
            ),

            Request::PublishResource(rsc_id, data) => self.controller.send(
                ActorId::Router,
                Message::Router(RouterCmd::ForwardToWorker {
                    source: id,
                    task: PersistenceCmd::Store(rsc_id, data),
                }),
            ),

            Request::RetrieveResource(rsc_id) => self.controller.send(
                ActorId::Router,
                Message::Router(RouterCmd::ForwardToWorker {
                    source: id,
                    task: PersistenceCmd::Retrieve(rsc_id),
                }),
            ),
        }
    }
}
