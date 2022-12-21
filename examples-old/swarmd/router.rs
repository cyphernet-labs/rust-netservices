use std::collections::{BTreeSet, HashMap, VecDeque};
use std::thread;
use std::time::Duration;

use re_actor::actors::IoEv;
use re_actor::{Actor, Controller, InternalError, ReactorApi};

use crate::daemon::{ActorId, Message};
use crate::p2p::P2pMsg;
use crate::persistence::{PersistenceCmd, PersistenceId};
use crate::rpc::Response;
use crate::{daemon, PeerId, ResourceId, RouteMap, Threads};

#[derive(Debug)]
pub enum RouterCmd {
    ListPeers(ActorId),
    ListLocalResources(ActorId),
    ResourceList(BTreeSet<ResourceId>, PersistenceId),
    ListAllResources(ActorId),
    ForwardToWorker {
        source: ActorId,
        task: PersistenceCmd,
    },
    WorkCompleted(PersistenceId, Box<Message>),
    RegisterPeer(PeerId),
    UnregisterPeer(PeerId),
}

pub struct RouterConfig {
    pub persistence_pool_size: u8,
}

#[derive(Copy, Clone, PartialEq, Eq, Hash, Debug, Display)]
#[display("")]
pub struct RouterId;

pub struct Router {
    routes: RouteMap,
    known_resources: BTreeSet<PeerId>,
    local_resources: BTreeSet<ResourceId>,
    controller: Controller<Threads>,
    free_workers: VecDeque<PersistenceId>,
    busy_workers: HashMap<PersistenceId, Option<ActorId>>,
    peers: BTreeSet<PeerId>,
}

impl Actor for Router {
    type Layout = Threads;
    type Id = RouterId;
    type Context = RouterConfig;
    type Cmd = RouterCmd;
    type Error = daemon::Error;

    fn with(
        context: Self::Context,
        mut controller: Controller<Self::Layout>,
    ) -> Result<Self, Self::Error>
    where
        Self: Sized,
    {
        let mut worker_ids = VecDeque::with_capacity(context.persistence_pool_size.into());
        for index in 0..context.persistence_pool_size.into() {
            controller.start_actor(Threads::Persistence, daemon::Context::Persistence(index))?;
            worker_ids.push_back(index);
        }

        let mut me = Self {
            routes: empty!(),
            known_resources: empty!(),
            local_resources: empty!(),
            controller,
            free_workers: worker_ids,
            busy_workers: empty!(),
            peers: empty!(),
        };

        me.start_work(PersistenceCmd::List, None)?;

        Ok(me)
    }

    fn id(&self) -> Self::Id {
        RouterId
    }

    fn io_ready(&mut self, io: IoEv) -> Result<(), Self::Error> {
        // Nothing to do here
        Ok(())
    }

    fn handle_cmd(&mut self, cmd: Self::Cmd) -> Result<(), Self::Error> {
        match cmd {
            RouterCmd::ListPeers(actor_id @ ActorId::Rpc(_)) => self
                .controller
                .send(actor_id, Message::Rpc(Response::Peers(self.peers.clone()))),
            RouterCmd::ResourceList(list, worker_id) => {
                self.local_resources = list;
                self.complete_work(worker_id, None)?;
                Ok(())
            }
            RouterCmd::ListLocalResources(actor_id @ ActorId::Rpc(_)) => self.controller.send(
                actor_id,
                Message::Rpc(Response::LocalResources(self.local_resources.clone())),
            ),
            RouterCmd::ListAllResources(actor_id @ ActorId::Rpc(_)) => self.controller.send(
                actor_id,
                Message::Rpc(Response::KnownResources(self.routes.clone())),
            ),
            RouterCmd::ListAllResources(actor_id @ ActorId::P2p(_)) => self
                .controller
                .send(actor_id, Message::R2p(P2pMsg::Routes(self.routes.clone()))),

            RouterCmd::ListPeers(actor_id)
            | RouterCmd::ListLocalResources(actor_id)
            | RouterCmd::ListAllResources(actor_id) => {
                log::error!(
                    "router received unexpected command {:?} from the actor {}",
                    cmd,
                    actor_id
                );
                Ok(())
            }

            RouterCmd::ForwardToWorker { source, task } => self.start_work(task, Some(source)),
            RouterCmd::WorkCompleted(worker_id, report) => {
                self.complete_work(worker_id, Some(*report))
            }

            RouterCmd::RegisterPeer(peer_id) => {
                if self.peers.insert(peer_id) {
                    log::error!("peer {} was already known to the router", peer_id);
                }
                // TODO: Return error here
                Ok(())
            }
            RouterCmd::UnregisterPeer(peer_id) => {
                if self.peers.remove(&peer_id) {
                    log::error!("peer {} was not known to the router", peer_id)
                }
                // TODO: Return error here
                Ok(())
            }
        }
        .map_err(daemon::Error::from)
    }

    fn handle_err(&mut self, err: Self::Error) -> Result<(), Self::Error> {
        log::error!("router failure: {}", err);
        Ok(())
    }
}

impl Router {
    fn start_work(
        &mut self,
        task: PersistenceCmd,
        source: Option<ActorId>,
    ) -> Result<(), InternalError<Threads>> {
        loop {
            if let Some(worker_id) = self.free_workers.pop_front() {
                self.busy_workers.insert(worker_id, source);
                self.controller
                    .send(ActorId::Persistence(worker_id), Message::Persistence(task))?;
                break;
            } else {
                thread::sleep(Duration::from_millis(10));
            }
        }
        Ok(())
    }

    fn complete_work(
        &mut self,
        worker_id: PersistenceId,
        report: Option<Message>,
    ) -> Result<(), InternalError<Threads>> {
        if let Some(source) = self.busy_workers.remove(&worker_id) {
            if let Some(actor_id) = source {
                self.controller.send(
                    actor_id,
                    report.expect(&format!("no report for actor {}", actor_id)),
                )?;
            }
        } else {
            log::error!(
                "worker {} reported to complete it job, but it was not listed as a busy",
                worker_id
            );
        }
        Ok(())
    }
}
