use std::collections::HashMap;
use std::time::{Duration, SystemTime};
use std::{io, net};

use crate::resources::tcp::TcpSocket;
use crate::resources::TcpConnector;
use crate::{InputEvent, Resource, ResourceMgr};

/// Maximum amount of time to wait for i/o.
const WAIT_TIMEOUT: Duration = Duration::from_secs(60 * 60);

#[derive(Debug)]
pub struct PollManager<R: Resource> {
    poll: popol::Poll<R::Addr>,
    resources: HashMap<R::Addr, R>,
    timeouts: TimeoutManager<()>,
}

impl PollManager<TcpConnector> {
    pub fn new(
        listen: &impl net::ToSocketAddrs,
        connect: &impl net::ToSocketAddrs,
    ) -> io::Result<Self> {
        let mut poll = popol::Poll::new();

        for addr in listen.to_socket_addrs()? {
            let socket = TcpSocket::listen(addr)?;
            let key = TcpConnector::Listen(addr);
            poll.register(key, &socket, popol::event::ALL);
        }

        for addr in connect.to_socket_addrs()? {
            let socket = TcpSocket::dial(addr)?;
            let key = TcpConnector::Connect(addr);
            poll.register(key, &socket, popol::event::ALL);
        }

        let timeouts = TimeoutManager::new(Duration::from_secs(1));

        Ok(PollManager { poll, timeouts })
    }
}

impl<R: Resource + Send + Sync> ResourceMgr<R> for PollManager<R> {
    type EventIterator = std::vec::IntoIter<InputEvent<R, Self::DisconnectReason>>;
    type DisconnectReason = ();

    fn get(&self, addr: &R::Addr) -> Option<&R> {
        self.poll.get_mut(addr)
    }

    fn connect_resource(&mut self, addr: &R::Addr) -> Result<bool, R::Error> {
        todo!()
    }

    fn disconnect_resource(
        &mut self,
        addr: &R::Addr,
        reason: Self::DisconnectReason,
    ) -> Result<bool, R::Error> {
        todo!()
    }

    fn register_resource(&mut self, resource: R) -> bool {
        todo!()
    }

    fn unregister_resource(&mut self, addr: &R::Addr) -> Option<R> {
        todo!()
    }

    fn try_read_events(&mut self) -> Result<Self::EventIterator, R::Error> {
        let timeout = self
            .timeouts
            .next(SystemTime::now())
            .unwrap_or(WAIT_TIMEOUT)
            .into();

        let mut events = Vec::new();
        let mut timeouts = Vec::with_capacity(32);

        // Blocking call
        if self.poll.wait_timeout(timeout)? {
            // Nb. The way this is currently used basically ignores which keys have
            // timed out. So as long as *something* timed out, we wake the service.
            self.timeouts.wake(local_time, &mut timeouts);

            if !timeouts.is_empty() {
                timeouts.clear();
                events.push(InputEvent::Timer);
            } else {
                events.push(InputEvent::Timeout);
            }
        }

        for (addr, ev) in self.poll.events() {
            if ev.is_writable() {
                self.handle_write(addr, &mut events);
            }
            if ev.is_readable() {
                self.handle_read(addr, &mut events);
            }
        }

        Ok(events.into_iter())
    }

    fn send(&mut self, addr: &R::Addr, data: impl AsRef<[u8]>) -> Result<usize, R::Error> {
        todo!()
    }
}

/// Manages timers and triggers timeouts.
#[derive(Debug)]
pub struct TimeoutManager<K> {
    timeouts: Vec<(K, SystemTime)>,
    threshold: Duration,
}

impl<K> TimeoutManager<K> {
    /// Create a new timeout manager.
    ///
    /// Takes a threshold below which two timeouts cannot overlap.
    pub fn new(threshold: Duration) -> Self {
        Self {
            timeouts: vec![],
            threshold,
        }
    }

    /// Return the number of timeouts being tracked.
    pub fn len(&self) -> usize {
        self.timeouts.len()
    }

    /// Check whether there are timeouts being tracked.
    pub fn is_empty(&self) -> bool {
        self.timeouts.is_empty()
    }

    /// Register a new timeout with an associated key and wake-up time.
    ///
    /// ```
    /// use nakamoto_net_poll::time::{LocalTime, LocalDuration, TimeoutManager};
    ///
    /// let mut tm = TimeoutManager::new(LocalDuration::from_secs(1));
    /// let now = LocalTime::now();
    ///
    /// let registered = tm.register(0xA, now + LocalDuration::from_secs(8));
    /// assert!(registered);
    ///
    /// let registered = tm.register(0xB, now + LocalDuration::from_secs(9));
    /// assert!(registered);
    /// assert_eq!(tm.len(), 2);
    ///
    /// let registered = tm.register(0xC, now + LocalDuration::from_millis(9541));
    /// assert!(!registered);
    ///
    /// let registered = tm.register(0xC, now + LocalDuration::from_millis(9999));
    /// assert!(!registered);
    /// assert_eq!(tm.len(), 2);
    /// ```
    pub fn register(&mut self, key: K, time: SystemTime) -> bool {
        // If this timeout is too close to a pre-existing timeout,
        // don't register it.
        if self
            .timeouts
            .iter()
            .any(|(_, t)| t.diff(time) < self.threshold)
        {
            return false;
        }

        self.timeouts.push((key, time));
        self.timeouts.sort_unstable_by(|(_, a), (_, b)| b.cmp(a));

        true
    }

    /// Get the minimum time duration we should wait for at least one timeout
    /// to be reached.  Returns `None` if there are no timeouts.
    ///
    /// ```
    /// use nakamoto_net_poll::time::{LocalTime, LocalDuration, TimeoutManager};
    ///
    /// let mut tm = TimeoutManager::new(LocalDuration::from_secs(0));
    /// let mut now = LocalTime::now();
    ///
    /// tm.register(0xA, now + LocalDuration::from_millis(16));
    /// tm.register(0xB, now + LocalDuration::from_millis(8));
    /// tm.register(0xC, now + LocalDuration::from_millis(64));
    ///
    /// // We need to wait 8 millis to trigger the next timeout (1).
    /// assert!(tm.next(now) <= Some(LocalDuration::from_millis(8)));
    ///
    /// // ... sleep for a millisecond ...
    /// now.elapse(LocalDuration::from_millis(1));
    ///
    /// // Now we don't need to wait as long!
    /// assert!(tm.next(now).unwrap() <= LocalDuration::from_millis(7));
    /// ```
    pub fn next(&self, now: impl Into<SystemTime>) -> Option<Duration> {
        let now = now.into();

        self.timeouts.last().map(|(_, t)| {
            if *t >= now {
                *t - now
            } else {
                Duration::from_secs(0)
            }
        })
    }

    /// Given the current time, populate the input vector with the keys that
    /// have timed out. Returns the number of keys that timed out.
    ///
    /// ```
    /// use nakamoto_net_poll::time::{LocalTime, LocalDuration, TimeoutManager};
    ///
    /// let mut tm = TimeoutManager::new(LocalDuration::from_secs(0));
    /// let now = LocalTime::now();
    ///
    /// tm.register(0xA, now + LocalDuration::from_millis(8));
    /// tm.register(0xB, now + LocalDuration::from_millis(16));
    /// tm.register(0xC, now + LocalDuration::from_millis(64));
    /// tm.register(0xD, now + LocalDuration::from_millis(72));
    ///
    /// let mut timeouts = Vec::new();
    ///
    /// assert_eq!(tm.wake(now + LocalDuration::from_millis(21), &mut timeouts), 2);
    /// assert_eq!(timeouts, vec![0xA, 0xB]);
    /// assert_eq!(tm.len(), 2);
    /// ```
    pub fn wake(&mut self, now: SystemTime, woken: &mut Vec<K>) -> usize {
        let before = woken.len();

        while let Some((k, t)) = self.timeouts.pop() {
            if now >= t {
                woken.push(k);
            } else {
                self.timeouts.push((k, t));
                break;
            }
        }
        woken.len() - before
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickcheck_macros::quickcheck;

    #[quickcheck]
    fn properties(timeouts: Vec<u64>, threshold: u64) -> bool {
        let threshold = LocalDuration::from_secs(threshold);
        let mut tm = TimeoutManager::new(threshold);
        let mut now = LocalTime::now();

        for t in timeouts {
            tm.register(t, now + LocalDuration::from_secs(t));
        }

        let mut woken = Vec::new();
        while let Some(delta) = tm.next(now) {
            now.elapse(delta);
            assert!(tm.wake(now, &mut woken) > 0);
        }

        let sorted = woken.windows(2).all(|w| w[0] <= w[1]);
        let granular = woken.windows(2).all(|w| w[1] - w[0] >= threshold.as_secs());

        sorted && granular
    }

    #[test]
    fn test_wake() {
        let mut tm = TimeoutManager::new(LocalDuration::from_secs(0));
        let now = LocalTime::now();

        tm.register(0xA, now + LocalDuration::from_millis(8));
        tm.register(0xB, now + LocalDuration::from_millis(16));
        tm.register(0xC, now + LocalDuration::from_millis(64));
        tm.register(0xD, now + LocalDuration::from_millis(72));

        let mut timeouts = Vec::new();

        assert_eq!(tm.wake(now, &mut timeouts), 0);
        assert_eq!(timeouts, vec![]);
        assert_eq!(tm.len(), 4);
        assert_eq!(
            tm.wake(now + LocalDuration::from_millis(9), &mut timeouts),
            1
        );
        assert_eq!(timeouts, vec![0xA]);
        assert_eq!(tm.len(), 3, "one timeout has expired");

        timeouts.clear();

        assert_eq!(
            tm.wake(now + LocalDuration::from_millis(66), &mut timeouts),
            2
        );
        assert_eq!(timeouts, vec![0xB, 0xC]);
        assert_eq!(tm.len(), 1, "another two timeouts have expired");

        timeouts.clear();

        assert_eq!(
            tm.wake(now + LocalDuration::from_millis(96), &mut timeouts),
            1
        );
        assert_eq!(timeouts, vec![0xD]);
        assert!(tm.is_empty(), "all timeouts have expired");
    }
}
