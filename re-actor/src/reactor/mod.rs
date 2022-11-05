mod controller;
mod error;
mod layout;
mod runtime;

use std::collections::HashMap;
use std::thread;
use std::thread::JoinHandle;

use crossbeam_channel as chan;

pub use controller::{Controller, ReactorApi};
pub use error::InternalError;
pub use layout::{Layout, Pool};

use self::runtime::{ControlEvent, PoolRuntime};
use crate::Scheduler;

/// Callbacks called in a context of the re-actor runtime threads.
pub trait Handler<L: Layout>: Send {
    /// Called on non-actor-specific errors - or on errors which were not held
    /// by the actors
    fn handle_err(&mut self, err: InternalError<L>);
}

/// Reactor, which provisioned with information about schedulers thread
/// [`Layout`] can run the re-actor runtime with [`Reactor::run`].
pub struct Reactor<L: Layout> {
    /// Threads running schedulers, one per pool.
    scheduler_threads: HashMap<L, JoinHandle<()>>,
    shutdown_send: chan::Sender<()>,
    shutdown_recv: chan::Receiver<()>,
    controller: Controller<L>,
    locked: bool,
}

impl<L: Layout> Reactor<L> {
    /// Constructs re-actor and runs it in a thread, returning [`Self`] as a
    /// controller exposing the API ([`ReactorApi`]).
    pub fn new() -> Result<Self, InternalError<L>>
    where
        L: 'static,
    {
        let (shutdown_send, shutdown_recv) = chan::bounded(1);

        let mut reactor = Reactor {
            scheduler_threads: empty!(),
            shutdown_send,
            shutdown_recv: shutdown_recv.clone(),
            controller: Controller::new(),
            locked: false,
        };

        let mut pools = Vec::new();

        // Required to avoid Actor: Send constraint
        struct Info<L: Layout> {
            id: L,
            scheduler: Box<dyn Scheduler<L::RootActor>>,
            control_recv: chan::Receiver<ControlEvent<L::RootActor>>,
            control_send: chan::Sender<ControlEvent<L::RootActor>>,
            shutdown: chan::Receiver<()>,
            handler: Box<dyn Handler<L>>,
        }

        for info in L::default_pools() {
            let (control_send, control_recv) = chan::unbounded();
            let shutdown = shutdown_recv.clone();
            let control = control_send.clone();

            pools.push(Info {
                id: info.id,
                scheduler: info.scheduler,
                control_recv,
                control_send,
                shutdown,
                handler: info.handler,
            });

            reactor.controller.register_pool(info.id, control)?;
        }

        for info in pools {
            let controller = reactor.controller();
            let id = info.id;
            let thread = thread::spawn(move || {
                PoolRuntime::new(
                    info.id,
                    info.scheduler,
                    info.control_recv,
                    info.control_send,
                    info.shutdown,
                    info.handler,
                )
                .run(controller)
            });
            reactor.scheduler_threads.insert(id, thread)
                .ok_or(())
                .expect("controller logic for pool management doesn't account for errors with repeated pool creation");
        }

        Ok(reactor)
    }

    /// Returns controller implementing [`ReactorApi`] for this re-actor.
    ///
    /// Once this function is called it wouldn't be possible to add more
    /// pools or actors to the re-actor (the re-actor state gets locked).
    pub fn controller(&mut self) -> Controller<L> {
        self.locked = true;
        self.controller.clone()
    }

    /// Joins all re-actor threads.
    pub fn join(self) -> Result<(), InternalError<L>> {
        for (pool, scheduler_thread) in self.scheduler_threads {
            scheduler_thread
                .join()
                .map_err(|_| InternalError::ThreadError(pool))?;
        }
        Ok(())
    }

    /// Shut downs the re-actor.
    pub fn shutdown(self) -> Result<(), InternalError<L>> {
        self.shutdown_send
            .send(())
            .map_err(|_| InternalError::ShutdownChanelBroken)?;
        self.join()?;
        Ok(())
    }
}
