use std::collections::VecDeque;
use std::io;
use std::os::unix::io::AsRawFd;
use std::time::Duration;

use crate::actors::{IoEv, IoSrc};
use crate::{Actor, Scheduler};

/// Manager for a set of reactor which are polled for an event loop by the
/// re-actor by using [`popol`] library.
pub struct PopolScheduler<R>
where
    R: Actor,
    R::Id: AsRawFd,
{
    poll: popol::Poll<R::Id>,
    events: VecDeque<IoSrc<R::Id>>,
}

impl<R> PopolScheduler<R>
where
    R: Actor,
    R::Id: AsRawFd,
{
    pub fn new() -> Self {
        Self {
            poll: popol::Poll::new(),
            events: empty!(),
        }
    }
}

impl<R> Scheduler<R> for PopolScheduler<R>
where
    R: Actor,
    R::Id: AsRawFd,
    R::Error: From<io::Error>,
{
    fn has_actor(&self, id: &R::Id) -> bool {
        self.poll.get(id).is_some()
    }

    fn register_actor(&mut self, resource: &R) -> Result<(), R::Error> {
        let id = resource.id();
        self.poll.register(id.clone(), &id, popol::event::ALL);
        Ok(())
    }

    fn unregister_actor(&mut self, id: &R::Id) -> Result<(), R::Error> {
        self.poll.unregister(id);
        Ok(())
    }

    fn wait_io(&mut self, timeout: Option<Duration>) -> Result<bool, R::Error> {
        // Blocking call
        if self.poll.wait_timeout(timeout.into())? {
            return Ok(true);
        }

        for (id, ev) in self.poll.events() {
            self.events.push_back(IoSrc {
                source: id.clone(),
                io: IoEv {
                    is_readable: ev.is_readable(),
                    is_writable: ev.is_writable(),
                },
            })
        }

        Ok(false)
    }
}

impl<R> Iterator for PopolScheduler<R>
where
    R: Actor,
    R::Id: AsRawFd,
{
    type Item = IoSrc<R::Id>;

    fn next(&mut self) -> Option<Self::Item> {
        self.events.pop_front()
    }
}
