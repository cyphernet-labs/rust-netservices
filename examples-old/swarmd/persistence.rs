use std::os::fd::RawFd;

use re_actor::actors::IoEv;
use re_actor::{Actor, Controller};

use crate::daemon::Threads;
use crate::{daemon, ResourceId};

#[derive(Copy, Clone, PartialEq, Eq, Hash, Getters, Debug, Display)]
#[display("{index}")]
pub struct PersistenceId {
    index: u64,
    fd: RawFd,
}

pub struct Persistence {
    id: PersistenceId,
    controller: Controller<Threads>,
}

#[derive(Debug)]
pub enum PersistenceCmd {
    List,
    Store(ResourceId, Vec<u8>),
    Retrieve(ResourceId),
}

impl Actor for Persistence {
    type Layout = Threads;
    type Id = PersistenceId;
    type Context = u64;
    type Cmd = PersistenceCmd;
    type Error = daemon::Error;

    fn with(
        context: Self::Context,
        controller: Controller<Self::Layout>,
    ) -> Result<Self, Self::Error>
    where
        Self: Sized,
    {
        // TODO: Open file etc
        Ok(Self {
            id: context,
            controller,
        })
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
