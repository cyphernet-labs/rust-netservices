use std::collections::VecDeque;
use std::io;
use std::os::unix::io::AsRawFd;
use std::time::Duration;

use crate::{IoManager, IoSrc, Resource};

/// Manager for a set of resources which are polled for an event loop by the
/// reactor by using [`popol`] library.
pub struct PollManager<R>
where
    R: Resource,
    R::Id: AsRawFd,
{
    poll: popol::Poll<R::Id>,
    events: VecDeque<IoSrc<R::Id>>,
}

impl<R> PollManager<R>
where
    R: Resource,
    R::Id: AsRawFd,
{
    pub fn new() -> Self {
        Self {
            poll: popol::Poll::new(),
            events: empty!(),
        }
    }
}

impl<R> IoManager<R> for PollManager<R>
where
    R: Resource,
    R::Id: AsRawFd,
    R::Error: From<io::Error>,
{
    fn has_resource(&self, id: &R::Id) -> bool {
        self.poll.get(id).is_some()
    }

    fn register_resource(&mut self, resource: &R) -> Result<(), R::Error> {
        let id = resource.id();
        self.poll.register(id.clone(), &id, popol::event::ALL);
        Ok(())
    }

    fn unregister_resource(&mut self, id: &R::Id) -> Result<(), R::Error> {
        self.poll.unregister(id);
        Ok(())
    }

    fn io_events(&mut self, timeout: Option<Duration>) -> Result<bool, R::Error> {
        // Blocking call
        if self.poll.wait_timeout(timeout.into())? {
            return Ok(true);
        }

        for (id, ev) in self.poll.events() {
            self.events.push_back(IoSrc {
                source: id.clone(),
                input: ev.is_readable(),
                output: ev.is_writable(),
            })
        }

        Ok(false)
    }
}

impl<R> Iterator for PollManager<R>
where
    R: Resource,
    R::Id: AsRawFd,
{
    type Item = IoSrc<R::Id>;

    fn next(&mut self) -> Option<Self::Item> {
        self.events.pop_front()
    }
}
