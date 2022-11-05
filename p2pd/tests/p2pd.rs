#[macro_use]
extern crate amplify;

use std::any::Any;
use std::error::Error as StdError;

use cyphernet::addr::LocalNode;
use cyphernet::crypto::ed25519::PrivateKey;
use ioreactor::schedulers::PopolScheduler;
use ioreactor::{Actor, Handler, InternalError, Layout, Pool, Reactor, ReactorApi};

use p2pd::peer::{Action, Context, PeerActor};

fn main() -> Result<(), Box<dyn StdError>> {
    let mut reactor = Reactor::<Microservices>::new()?;

    let nsh_socket = Context {
        method: Action::Connect("127.0.0.1".parse().unwrap()),
        local_node: LocalNode::from(PrivateKey::test()),
    };
    reactor.start_actor(Microservices::Peer, nsh_socket)?;
    reactor.join().unwrap();
    Ok(())
}

#[derive(Copy, Clone, Eq, PartialEq, Hash, Display, Debug)]
#[display(lowercase)]
enum Microservices {
    Peer = 0,
    Worker = 1,
}

const PEER_POOL: u32 = Microservices::Peer as u32;

impl Layout for Microservices {
    type RootActor = DaemonActor;

    fn default_pools() -> Vec<Pool<DaemonActor, Self>> {
        vec![Pool::new(
            Microservices::Peer,
            PopolScheduler::<DaemonActor>::new(),
            Service,
        )]
    }

    fn convert(other_ctx: Box<dyn Any>) -> <Self::RootActor as Actor>::Context {
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
            x if x == Microservices::Peer as u32 => Microservices::Peer,
            x if x == Microservices::Worker as u32 => Microservices::Worker,
            _ => panic!("invalid daemon pool id {}", value),
        }
    }
}

impl From<Microservices> for u32 {
    fn from(value: Microservices) -> Self {
        value as u32
    }
}

type DaemonActor = PeerActor<Microservices, PEER_POOL>;

struct Service;

impl Handler<Microservices> for Service {
    fn handle_err(&mut self, err: InternalError<Microservices>) {
        panic!("{}", err);
    }
}
