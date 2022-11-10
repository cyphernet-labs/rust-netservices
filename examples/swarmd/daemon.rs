use std::any::Any;

use reactor::actors::IoEv;
use reactor::schedulers::{CrossbeamScheduler, PopolScheduler};
use reactor::{Actor, Controller, InternalError, Layout, Pool};

use crate::p2p::{P2pActor, P2pMsg};
use crate::persistence::{Persistence, PersistenceCmd, PersistenceId};
use crate::router::{Router, RouterCmd};
use crate::rpc::{ClientId, Response, RpcActor};
use crate::PeerId;

#[derive(Copy, Clone, Eq, PartialEq, Hash, Display, Debug)]
#[display(lowercase)]
pub enum Microservices {
    P2p = 0,
    Rpc = 1,
    Router = 10,
    Persistence = 11,
}

pub const P2P_THREAD: u32 = Microservices::P2p as u32;
pub const RPC_THREAD: u32 = Microservices::Rpc as u32;
pub const ROUTER_THREAD: u32 = Microservices::Router as u32;
pub const PERSISTENCE_THREAD: u32 = Microservices::Persistence as u32;

impl Layout for Microservices {
    type RootActor = DaemonActor;

    fn default_pools() -> Vec<Pool<DaemonActor, Self>> {
        vec![
            Pool::new(
                Microservices::P2p,
                PopolScheduler::<DaemonActor>::new(),
                Handler,
            ),
            Pool::new(
                Microservices::Rpc,
                PopolScheduler::<DaemonActor>::new(),
                Handler,
            ),
            Pool::new(
                Microservices::Router,
                CrossbeamScheduler::<DaemonActor>::new(),
                Handler,
            ),
            Pool::new(
                Microservices::Persistence,
                PopolScheduler::<DaemonActor>::new(),
                Handler,
            ),
        ]
    }

    fn convert(other_ctx: Box<dyn Any>) -> <Self::RootActor as Actor>::Context {
        // TODO: Support context deduction for all sub-actors
        let ctx = other_ctx
            .downcast::<Context>()
            .expect("wrong context object");
        let ctx = *ctx;
        ctx.into()
    }
}

impl From<u32> for Microservices {
    fn from(value: u32) -> Self {
        match value {
            x if x == Microservices::P2p as u32 => Microservices::P2p,
            x if x == Microservices::Rpc as u32 => Microservices::Rpc,
            x if x == Microservices::Router as u32 => Microservices::Router,
            x if x == Microservices::Persistence as u32 => Microservices::Persistence,
            _ => panic!("invalid daemon pool id {}", value),
        }
    }
}

impl From<Microservices> for u32 {
    fn from(value: Microservices) -> Self {
        value as u32
    }
}

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Display)]
pub enum ActorId {
    #[display("p2p({0})")]
    P2p(PeerId),

    #[display("rpc({0})")]
    Rpc(ClientId),

    #[display("router")]
    Router,

    #[display("persistence({0})")]
    Persistence(PersistenceId),
}

#[derive(Debug)]
pub enum Message {
    R2p(P2pMsg),
    Rpc(Response),
    Router(RouterCmd),
    Persistence(PersistenceCmd),
}

pub enum Context {
    R2p(<P2pActor as Actor>::Context),
    Rpc(<RpcActor as Actor>::Context),
    Router(<Router as Actor>::Context),
    Persistence(<Persistence as Actor>::Context),
}

pub enum DaemonActor {
    P2p(P2pActor),
    Rpc(RpcActor),
    Router(Router),
    Persistence(Persistence),
}

impl Actor for DaemonActor {
    type Layout = Microservices;
    type Id = ActorId;
    type Context = Context;
    type Cmd = Message;
    type Error = Error;

    fn with(
        context: Self::Context,
        controller: Controller<Self::Layout>,
    ) -> Result<Self, Self::Error>
    where
        Self: Sized,
    {
        match context {
            Context::R2p(ctx) => P2pActor::with(ctx, controller).map(Self::P2p),
            Context::Rpc(ctx) => RpcActor::with(ctx, controller).map(Self::Rpc),
            Context::Router(ctx) => Router::with(ctx, controller).map(Self::Router),
            Context::Persistence(ctx) => Persistence::with(ctx, controller).map(Self::Persistence),
        }
        .map_err(Error::from)
    }

    fn id(&self) -> Self::Id {
        match self {
            DaemonActor::P2p(actor) => ActorId::P2p(actor.id()),
            DaemonActor::Rpc(actor) => ActorId::Rpc(actor.id()),
            DaemonActor::Router(_) => ActorId::Router,
            DaemonActor::Persistence(actor) => ActorId::Persistence(actor.id()),
        }
    }

    fn io_ready(&mut self, io: IoEv) -> Result<(), Self::Error> {
        match self {
            DaemonActor::P2p(actor) => actor.io_ready(io).map_err(Error::from),
            DaemonActor::Rpc(actor) => actor.io_ready(io),
            DaemonActor::Router(actor) => actor.io_ready(io),
            DaemonActor::Persistence(actor) => actor.io_ready(io),
        }
    }

    fn handle_cmd(&mut self, cmd: Self::Cmd) -> Result<(), Self::Error> {
        match (self, cmd) {
            (Self::P2p(actor), Message::R2p(msg)) => actor.handle_cmd(msg).map_err(Error::from),
            (Self::Rpc(actor), Message::Rpc(msg)) => actor.handle_cmd(msg),
            (Self::Router(actor), Message::Router(msg)) => actor.handle_cmd(msg),
            (Self::Persistence(actor), Message::Persistence(msg)) => actor.handle_cmd(msg),
            (_, cmd) => panic!("actor {} called with invalid command {:?}", self.id(), cmd),
        }
    }

    fn handle_err(&mut self, err: Self::Error) -> Result<(), Self::Error> {
        match self {
            DaemonActor::P2p(actor) => actor.handle_err(err),
            DaemonActor::Rpc(actor) => actor.handle_err(err),
            DaemonActor::Router(actor) => actor.handle_err(err),
            DaemonActor::Persistence(actor) => actor.handle_err(err),
        }
    }
}

pub struct Handler;

impl reactor::Handler<Microservices> for Handler {
    fn handle_err(&mut self, err: InternalError<Microservices>) {
        panic!("{}", err);
    }
}

#[derive(Debug, Display, Error, From)]
#[display(inner)]
pub enum Error {
    #[from(InternalError<Threads>)]
    Internal(Box<InternalError<Microservices>>),
}
