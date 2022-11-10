use std::collections::VecDeque;
use std::time::Duration;

use crossbeam_channel as chan;

use crate::actors::crossbeam::CrossbeamActor;
use crate::actors::{IoEv, IoSrc};
use crate::Scheduler;

/// Scheduler which is able to run actor not depending on the I/O events
/// other than in-memory channels;
pub struct CrossbeamScheduler<R: CrossbeamActor<T>, T: Send>
where
    R::Cmd: From<T>,
{
    events: VecDeque<IoSrc<R::Id>>,
    channels: Vec<chan::Receiver<T>>,
}

impl<R: CrossbeamActor<T>, T: Send> CrossbeamScheduler<R, T>
where
    R::Cmd: From<T>,
{
    pub fn new() -> Self {
        Self {
            events: empty!(),
            channels: none!(),
        }
    }
}

impl<R: CrossbeamActor<T>, T: Send> Scheduler<R> for CrossbeamScheduler<R, T>
where
    R::Cmd: From<T>,
{
    fn has_actor(&self, id: &R::Id) -> bool {
        self.channels.contains(id)
    }

    fn register_actor(&mut self, actor: &R) -> Result<(), R::Error> {
        self.channels.push(actor.id());
        Ok(())
    }

    fn unregister_actor(&mut self, id: &R::Id) -> Result<(), R::Error> {
        self.channels
            .iter()
            .position(|a| a == *id)
            .and_then(|pos| self.channels.remove(pos));
        Ok(())
    }

    fn wait_io(&mut self, timeout: Option<Duration>) -> Result<bool, R::Error> {
        // Build a list of operations.
        let mut sel = chan::Select::new();
        for r in &self.channels {
            sel.recv(r);
        }

        let oper = match timeout {
            Some(tout) => match sel.select_timeout(tout) {
                Ok(o) => o,
                Err(_) => return Ok(true),
            },
            None => sel.select(),
        };
        let index = oper.index();
        self.events.push_back(IoSrc {
            source: self.channels[index].clone(),
            io: IoEv {
                is_readable: true,
                is_writable: false,
            },
        });
        Ok(false)
    }
}

impl<R: CrossbeamActor<T>, T: Send> Iterator for CrossbeamScheduler<R, T>
where
    R::Cmd: From<T>,
{
    type Item = IoSrc<R::IoResource>;

    fn next(&mut self) -> Option<Self::Item> {
        self.events.pop_front()
    }
}
