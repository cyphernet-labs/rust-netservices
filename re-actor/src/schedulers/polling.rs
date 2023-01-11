use polling::{Event, Poller, Source};
use std::collections::{HashSet, VecDeque};
use std::io;
use std::os::unix::io::{FromRawFd, RawFd};
use std::time::Duration;

use crate::actors::{IoEv, IoSrc};
use crate::{Actor, Scheduler};

/// Manager for a set of reactor which are polled for an event loop by the
/// re-actor by using [`polling`] library.
pub struct PollingScheduler<R>
where
    R: Actor,
    R::Id: Source,
{
    poll: Poller,
    actors: HashSet<R::Id>,
    events: VecDeque<IoSrc<R::Id>>,
    read_events: Vec<Event>,
}

impl<R> PollingScheduler<R>
where
    R: Actor,
    R::Id: Source,
{
    pub fn new() -> io::Result<Self> {
        Ok(Self {
            poll: Poller::new()?,
            actors: empty!(),
            events: empty!(),
            read_events: empty!(),
        })
    }
}

impl<R> Scheduler<R> for PollingScheduler<R>
where
    R: Actor,
    R::Id: Source + FromRawFd,
    R::Error: From<io::Error>,
{
    fn has_actor(&self, id: &R::Id) -> bool {
        self.actors.contains(id)
    }

    fn register_actor(&mut self, resource: &R) -> Result<(), R::Error> {
        let id = resource.id();
        let raw = id.raw();
        self.poll.add(id, Event::all(raw as usize))?;
        self.actors.insert(resource.id());
        Ok(())
    }

    fn unregister_actor(&mut self, id: &R::Id) -> Result<(), R::Error> {
        self.actors.remove(id);
        self.poll.delete(id.raw())?;
        Ok(())
    }

    fn wait_io(&mut self, timeout: Option<Duration>) -> Result<bool, R::Error> {
        // Blocking call
        self.poll.wait(&mut self.read_events, timeout)?;

        if self.read_events.is_empty() {
            return Ok(true);
        }

        for ev in &self.read_events {
            self.events.push_back(IoSrc {
                source: unsafe { R::Id::from_raw_fd(ev.key as RawFd) },
                io: IoEv {
                    is_readable: ev.readable,
                    is_writable: ev.writable,
                },
            })
        }
        self.read_events.clear();

        Ok(false)
    }
}

impl<R> Iterator for PollingScheduler<R>
where
    R: Actor,
    R::Id: Source,
{
    type Item = IoSrc<R::Id>;

    fn next(&mut self) -> Option<Self::Item> {
        self.events.pop_front()
    }
}
