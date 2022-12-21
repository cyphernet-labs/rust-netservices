use std::collections::BTreeSet;

use netservices::peer;
use netservices::peer::PeerActor;
use re_actor::actors::IoEv;
use re_actor::{Actor, Controller};

use crate::daemon::P2P_THREAD;
use crate::{daemon, PeerId, ResourceId, RouteMap, Threads};

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
    peer: PeerActor<Threads, P2P_THREAD>,
}

impl Actor for P2pActor {
    type Layout = Threads;
    type Id = PeerId;
    type Context = peer::Context;
    type Cmd = P2pMsg;
    type Error = daemon::Error;

    fn with(
        context: Self::Context,
        controller: Controller<Self::Layout>,
    ) -> Result<Self, Self::Error>
    where
        Self: Sized,
    {
        todo!()
    }

    fn id(&self) -> Self::Id {
        todo!()
    }

    fn io_ready(&mut self, io: IoEv) -> Result<(), Self::Error> {
        todo!()
    }

    fn handle_cmd(&mut self, cmd: Self::Cmd) -> Result<(), Self::Error> {
        todo!()
    }

    fn handle_err(&mut self, err: Self::Error) -> Result<(), Self::Error> {
        todo!()
    }
}
