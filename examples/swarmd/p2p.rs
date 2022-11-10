use std::collections::BTreeSet;
use std::io;

use netservices::peer;
use netservices::peer::PeerActor;
use reactor::actors::IoEv;
use reactor::{Actor, Controller, ReactorApi};

use crate::daemon::{ActorId, Message, P2P_THREAD};
use crate::router::RouterCmd;
use crate::{daemon, Microservices, PeerId, ResourceId, RouteMap};

#[derive(Debug)]
pub enum P2pMsg {
    // Gossip protocol
    AnnouncePeer(PeerId, BTreeSet<ResourceId>),
    AnnounceResource(PeerId, ResourceId),
    OfflinePeer(PeerId),

    // P2P-only protocol
    GetRoutes,
    Routes(RouteMap),
    FetchResource(ResourceId),
    PostResource(ResourceId, Vec<u8>),
    Error(String),
}

pub struct P2pActor {
    peer: PeerActor<Microservices, P2P_THREAD>,
    controller: Controller<Microservices>,
}

impl Actor for P2pActor {
    type Layout = Microservices;
    type Id = PeerId;
    type Context = peer::Context;
    type Cmd = P2pMsg;
    type Error = io::Error;

    fn with(
        context: Self::Context,
        controller: Controller<Self::Layout>,
    ) -> Result<Self, Self::Error>
    where
        Self: Sized,
    {
        PeerActor::with(context, controller).map(Self)
    }

    fn id(&self) -> Self::Id {
        self.0.id()
    }

    fn io_ready(&mut self, io: IoEv) -> Result<(), Self::Error> {
        if io.is_readable {
            self.handle_read()
        }
    }

    fn handle_cmd(&mut self, cmd: Self::Cmd) -> Result<(), Self::Error> {}

    fn handle_err(&mut self, err: Self::Error) -> Result<(), Self::Error> {
        self.0.handle_err(err)
    }
}

impl P2pActor {
    fn handle_read(&self) {
        self.controller.send(
            ActorId::Router,
            Message::Router(RouterCmd::ForwardToWorker {}),
        )
    }
}
