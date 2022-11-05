#[macro_use]
extern crate amplify;

use std::any::Any;
use std::error::Error as StdError;

use cyphernet::addr::LocalNode;
use cyphernet::crypto::ed25519::PrivateKey;
use ioreactor::schedulers::PopolScheduler;
use ioreactor::{Actor, Handler, InternalError, Pool, PoolInfo, Reactor, ReactorApi};

use p2pd::peer::{Action, Context, PeerActor};

fn main() -> Result<(), Box<dyn StdError>> {
    let mut reactor = Reactor::<DaemonPool>::new()?;

    let nsh_socket = Context {
        method: Action::Connect("127.0.0.1".parse().unwrap()),
        local_node: LocalNode::from(PrivateKey::test()),
    };
    reactor.start_actor(DaemonPool::Peer, nsh_socket)?;
    reactor.join().unwrap();
    Ok(())
}

#[derive(Copy, Clone, Eq, PartialEq, Hash, Display, Debug)]
#[display(lowercase)]
enum DaemonPool {
    Peer = 0,
}

const PEER_POOL: u32 = DaemonPool::Peer as u32;

impl Pool for DaemonPool {
    type RootActor = DaemonActor;

    fn default_pools() -> Vec<PoolInfo<DaemonActor, Self>> {
        vec![PoolInfo::new(
            DaemonPool::Peer,
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

impl From<u32> for DaemonPool {
    fn from(value: u32) -> Self {
        DaemonPool::Peer
    }
}

impl From<DaemonPool> for u32 {
    fn from(value: DaemonPool) -> Self {
        value as u32
    }
}

type DaemonActor = PeerActor<DaemonPool, PEER_POOL>;

struct Service;

impl Handler<DaemonPool> for Service {
    fn handle_err(&mut self, err: InternalError<DaemonPool>) {
        panic!("{}", err);
    }
}
