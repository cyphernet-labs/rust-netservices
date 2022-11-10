use crossbeam_channel as chan;
use std::collections::HashMap;
use std::time::{Duration, Instant};

use crate::{Actor, Controller, Handler, InternalError, Layout, Scheduler, TimeoutManager};

/// Events send by [`Controller`] and [`ReactorApi`] to the [`Runtime`].
#[derive(Clone, Ord, PartialOrd, Eq, PartialEq, Hash, Debug)]
pub enum ControlEvent<A: Actor> {
    /// Request re-actor to connect to the resource with some context
    RunActor(A::Context),

    /// Request re-actor to disconnect from a resource
    StopActor(A::Id),

    /// Ask re-actor to wake up after certain interval
    SetTimer(),

    /// Request re-actor to send the data to the resource
    Send(A::Id, A::Cmd),
}

/// Runtime represents the re-actor event loop with its state handled in a
/// dedicated thread by the re-actor. It is controlled by sending instructions
/// through a set of crossbeam channels. [`Reactor`] abstracts that control via
/// exposing high-level [`ReactorApi`] and [`Controller`] objects.
pub struct PoolRuntime<L: Layout> {
    id: L,
    actors: HashMap<<L::RootActor as Actor>::Id, L::RootActor>,
    scheduler: Box<dyn Scheduler<L::RootActor>>,
    handler: Box<dyn Handler<L>>,
    control_recv: chan::Receiver<ControlEvent<L::RootActor>>,
    control_send: chan::Sender<ControlEvent<L::RootActor>>,
    shutdown: chan::Receiver<()>,
    timeouts: TimeoutManager<()>,
}

impl<L: Layout> PoolRuntime<L> {
    pub fn new(
        id: L,
        scheduler: Box<dyn Scheduler<L::RootActor>>,
        control_recv: chan::Receiver<ControlEvent<L::RootActor>>,
        control_send: chan::Sender<ControlEvent<L::RootActor>>,
        shutdown: chan::Receiver<()>,
        handler: Box<dyn Handler<L>>,
    ) -> Self {
        PoolRuntime {
            id,
            scheduler,
            actors: empty!(),
            control_recv,
            control_send,
            shutdown,
            handler,
            timeouts: TimeoutManager::new(Duration::from_secs(0)),
        }
    }

    pub fn run(mut self, controller: Controller<L>) -> ! {
        loop {
            let now = Instant::now();
            if let Err(err) = self.scheduler.wait_io(self.timeouts.next(now)) {
                self.handler
                    .handle_err(InternalError::ActorError(self.id, err));
            }
            for ev in &mut self.scheduler {
                let res = self
                    .actors
                    .get_mut(&ev.source)
                    .expect("resource management inconsistency");
                res.io_ready(ev.io)
                    .or_else(|err| res.handle_err(err))
                    .unwrap_or_else(|err| {
                        self.handler
                            .handle_err(InternalError::ActorError(self.id, err))
                    });
            }
            // TODO: Should we process control events before dispatching input?
            self.process_control(&controller);
            self.process_shutdown();
        }
    }

    fn process_control(&mut self, controller: &Controller<L>) {
        loop {
            match self.control_recv.try_recv() {
                Err(chan::TryRecvError::Disconnected) => {
                    panic!("re-actor shutdown channel was dropper")
                }
                Err(chan::TryRecvError::Empty) => break,
                Ok(event) => match event {
                    ControlEvent::RunActor(context) => {
                        match L::RootActor::with(context, controller.clone()) {
                            Err(err) => self
                                .handler
                                .handle_err(InternalError::ActorError(self.id, err)),
                            Ok(mut resource) => {
                                self.scheduler
                                    .register_actor(&resource)
                                    .or_else(|err| resource.handle_err(err))
                                    .unwrap_or_else(|err| {
                                        self.handler
                                            .handle_err(InternalError::ActorError(self.id, err))
                                    });
                                self.actors.insert(resource.id(), resource);
                            }
                        };
                        // TODO: Consider to error to the user if the resource was already present
                    }
                    ControlEvent::StopActor(id) => {
                        self.scheduler.unregister_actor(&id).unwrap_or_else(|err| {
                            self.handler
                                .handle_err(InternalError::ActorError(self.id, err))
                        });
                        self.actors.remove(&id);
                        // TODO: Don't we need to shutdown the resource?
                    }
                    ControlEvent::SetTimer() => {
                        // TODO: Add timeout manager
                    }
                    ControlEvent::Send(id, data) => {
                        if let Some(resource) = self.actors.get_mut(&id) {
                            resource
                                .handle_cmd(data)
                                .or_else(|err| resource.handle_err(err))
                                .unwrap_or_else(|err| {
                                    self.handler
                                        .handle_err(InternalError::ActorError(self.id, err))
                                });
                        }
                    }
                },
            }
        }
    }

    fn process_shutdown(&mut self) {
        match self.shutdown.try_recv() {
            Err(chan::TryRecvError::Empty) => {
                // Nothing to do here
            }
            Ok(()) => {
                // TODO: Disconnect all resources
            }
            Err(chan::TryRecvError::Disconnected) => {
                panic!("re-actor shutdown channel was dropper")
            }
        }
    }
}
