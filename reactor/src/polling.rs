use polling::{Event, Poller, Source};
use std::collections::{HashSet, VecDeque};
use std::io;
use std::os::unix::io::{FromRawFd, RawFd};
use std::time::Duration;

use crate::{IoManager, IoSrc, Resource};

/// Manager for a set of resources which are polled for an event loop by the
/// reactor by using [`polling`] library.
pub struct PollManager<R>
where
    R: Resource,
    R::Id: Source,
{
    poll: Poller,
    resources: HashSet<R::Id>,
    events: VecDeque<IoSrc<R::Id>>,
    read_events: Vec<Event>,
}

impl<R> PollManager<R>
where
    R: Resource,
    R::Id: Source,
{
    pub fn new() -> io::Result<Self> {
        Ok(Self {
            poll: Poller::new()?,
            resources: empty!(),
            events: empty!(),
            read_events: empty!(),
        })
    }
}

impl<R> IoManager<R> for PollManager<R>
where
    R: Resource,
    R::Id: Source + FromRawFd,
    R::Error: From<io::Error>,
{
    fn has_resource(&self, id: &R::Id) -> bool {
        self.resources.contains(id)
    }

    fn register_resource(&mut self, resource: &R) -> Result<(), R::Error> {
        let id = resource.id();
        let raw = id.raw();
        self.poll.add(id, Event::all(raw as usize))?;
        self.resources.insert(resource.id());
        Ok(())
    }

    fn unregister_resource(&mut self, id: &R::Id) -> Result<(), R::Error> {
        self.resources.remove(id);
        self.poll.delete(id.raw())?;
        Ok(())
    }

    fn io_events(&mut self, timeout: Option<Duration>) -> Result<bool, R::Error> {
        // Blocking call
        self.poll.wait(&mut self.read_events, timeout)?;

        if self.read_events.is_empty() {
            return Ok(true);
        }

        for ev in &self.read_events {
            self.events.push_back(IoSrc {
                source: unsafe { R::Id::from_raw_fd(ev.key as RawFd) },
                input: ev.readable,
                output: ev.writable,
            })
        }
        self.read_events.clear();

        Ok(false)
    }
}

impl<R> Iterator for PollManager<R>
where
    R: Resource,
    R::Id: Source,
{
    type Item = IoSrc<R::Id>;

    fn next(&mut self) -> Option<Self::Item> {
        self.events.pop_front()
    }
}
