use std::collections::{HashMap, VecDeque};
use std::io;
use std::os::unix::io::{FromRawFd, RawFd};
use std::time::Duration;

use polling::{Event, Poller, Source};

use crate::actors::{IoEv, IoSrc};
use crate::{Actor, Scheduler};

/// Manager for a set of resources which are polled for an event loop by the
/// re-actor by using [`polling`] library.
pub struct PollingScheduler<'r, R>
where
    R: Actor,
    R::IoResource: Source,
{
    poll: Poller,
    actors: HashMap<R::Id, &'r R::IoResource>,
    events: VecDeque<IoSrc<R::Id>>,
    read_events: Vec<Event>,
}

impl<'r, R> PollingScheduler<'r, R>
where
    R: Actor,
    R::IoResource: Source,
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

impl<'r, R> Scheduler<R> for PollingScheduler<'r, R>
where
    R: Actor,
    R::IoResource: Source + FromRawFd + Sync,
    R::Error: From<io::Error>,
{
    fn has_actor(&self, id: &R::Id) -> bool {
        self.actors.contains(id)
    }

    fn register_actor(&mut self, actor: &R) -> Result<(), R::Error> {
        let rsc = actor.io_resource();
        let raw = rsc.raw();
        self.poll.add(raw, Event::all(raw as usize))?;
        self.actors.insert(actor.id(), rsc);
        Ok(())
    }

    fn unregister_actor(&mut self, id: &R::Id) -> Result<(), R::Error> {
        if let Some(rsc) = self.actors.remove(id) {
            self.poll.delete(rsc)?;
        }
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

impl<'r, R> Iterator for PollingScheduler<'r, R>
where
    R: Actor,
    R::IoResource: Source,
{
    type Item = IoSrc<R::IoResource>;

    fn next(&mut self) -> Option<Self::Item> {
        self.events.pop_front()
    }
}
