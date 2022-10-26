use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::Hash;
use std::time::{Duration, Instant};
use std::{io, net};

use crate::resources::tcp::TcpSocket;
use crate::resources::{FdResource, TcpConnector};
use crate::{InputEvent, OnDemand, Resource, ResourceMgr};

/// Maximum amount of time to wait for i/o.
const WAIT_TIMEOUT: Duration = Duration::from_secs(60 * 60);

pub struct PollManager<R: Resource>
where
    R::Addr: Hash,
{
    poll: popol::Poll<R::Addr>,
    connecting: HashSet<R::Addr>,
    events: VecDeque<InputEvent<R>>,
    resources: HashMap<R::Addr, R>,
    timeouts: TimeoutManager<()>,
}

impl PollManager<TcpSocket> {
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

        Ok(PollManager {
            poll,
            connecting: empty!(),
            events: empty!(),
            resources: empty!(),
            timeouts,
        })
    }
}

impl<'me, R> ResourceMgr<R> for PollManager<R>
where
    Self: 'me,
    R: FdResource + Send + Sync,
    R::Addr: Hash,
    R::DisconnectReason: Send,
    R::Error: From<io::Error>,
{
    type EventIterator = Self;

    fn get(&self, addr: &R::Addr) -> Option<&R> {
        self.resources.get(addr)
    }

    fn connect_resource(&mut self, addr: &R::Addr) -> Result<bool, R::Error> {
        if self.has(addr) || self.connecting.contains(addr) {
            return Ok(false);
        }
        if self.connecting.contains(addr) {
            return Ok(false);
        }
        let res = R::connect(addr)?;
        Ok(self.register_resource(res))
    }

    fn disconnect_resource(&mut self, addr: &R::Addr) -> Result<bool, R::Error> {
        if !self.has(addr) || self.connecting.contains(addr) {
            return Ok(false);
        }
        self.resources
            .get_mut(addr)
            .expect("broken resource index")
            .disconnect()?;

        let disconnection_event =
            InputEvent::<R>::Disconnected(addr.to_owned(), R::DisconnectReason::on_demand());
        self.events.push_back(disconnection_event);

        Ok(self.unregister_resource(addr).is_some())
    }

    fn register_resource(&mut self, resource: R) -> bool {
        self.resources.insert(resource.addr(), resource).is_some()
    }

    fn unregister_resource(&mut self, addr: &R::Addr) -> Option<R> {
        self.resources.remove(addr)
    }

    fn read_events(&mut self) -> Result<&mut Self::EventIterator, R::Error> {
        let timeout = self
            .timeouts
            .next(Instant::now())
            .unwrap_or(WAIT_TIMEOUT)
            .into();

        let mut timeouts = Vec::with_capacity(32);

        // Blocking call
        if self.poll.wait_timeout(timeout)? {
            // Nb. The way this is currently used basically ignores which keys have
            // timed out. So as long as *something* timed out, we wake the service.
            self.timeouts.check_now(&mut timeouts);

            if !timeouts.is_empty() {
                timeouts.clear();
                self.events.push_back(InputEvent::Timer);
            } else {
                self.events.push_back(InputEvent::Timeout);
            }
        }

        for (addr, ev) in self.poll.events() {
            let src = self.resources.get_mut(addr).expect("broken resource index");
            let mut events = Vec::with_capacity(2);
            if ev.is_writable() {
                events.push(src.read_output_event()?);
            }
            if ev.is_readable() {
                events.push(src.read_input_event()?);
            }

            for event in &events {
                if let InputEvent::Connected { remote_addr, .. } = event {
                    debug_assert!(
                        !self.connecting.remove(remote_addr),
                        "broken connection management"
                    );
                }
            }

            self.events.extend(events);
        }

        Ok(self)
    }

    fn events(&mut self) -> &mut Self::EventIterator {
        self
    }

    fn into_events(self) -> Self::EventIterator {
        self
    }

    fn send(&mut self, addr: &R::Addr, data: impl AsRef<[u8]>) -> Result<usize, R::Error> {
        self.resources
            .get_mut(addr)
            .expect("broken resource index")
            .write(data.as_ref())
            .map_err(R::Error::from)
    }
}

impl<R> Iterator for PollManager<R>
where
    R: Resource,
    R::Addr: Hash,
{
    type Item = InputEvent<R>;

    fn next(&mut self) -> Option<Self::Item> {
        self.events.pop_front()
    }
}

/// Manages timers and triggers timeouts.
#[derive(Debug)]
pub struct TimeoutManager<K> {
    timeouts: Vec<(K, Instant)>,
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
    /// use std::time::{Duration, Instant};
    /// use reactor::managers::TimeoutManager;
    ///
    /// let mut tm = TimeoutManager::new(Duration::from_secs(1));
    /// let now = Instant::now();
    ///
    /// let registered = tm.register(0xA, now + Duration::from_secs(8));
    /// assert!(registered);
    ///
    /// let registered = tm.register(0xB, now + Duration::from_secs(9));
    /// assert!(registered);
    /// assert_eq!(tm.len(), 2);
    ///
    /// let registered = tm.register(0xC, now + Duration::from_millis(9541));
    /// assert!(!registered);
    ///
    /// let registered = tm.register(0xC, now + Duration::from_millis(9999));
    /// assert!(!registered);
    /// assert_eq!(tm.len(), 2);
    /// ```
    pub fn register(&mut self, key: K, time: Instant) -> bool {
        // If this timeout is too close to a pre-existing timeout,
        // don't register it.
        if self.timeouts.iter().any(|(_, t)| {
            if *t < time {
                time.duration_since(*t) < self.threshold
            } else {
                t.duration_since(time) < self.threshold
            }
        }) {
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
    /// # use std::time::{Duration, Instant};
    /// use reactor::managers::TimeoutManager;
    ///
    /// let mut tm = TimeoutManager::new(Duration::from_secs(0));
    /// let mut now = Instant::now();
    ///
    /// tm.register(0xA, now + Duration::from_millis(16));
    /// tm.register(0xB, now + Duration::from_millis(8));
    /// tm.register(0xC, now + Duration::from_millis(64));
    ///
    /// // We need to wait 8 millis to trigger the next timeout (1).
    /// assert!(tm.next(now) <= Some(Duration::from_millis(8)));
    ///
    /// // ... sleep for a millisecond ...
    /// now += Duration::from_millis(1);
    ///
    /// // Now we don't need to wait as long!
    /// assert!(tm.next(now).unwrap() <= Duration::from_millis(7));
    /// ```
    pub fn next(&self, now: impl Into<Instant>) -> Option<Duration> {
        let now = now.into();

        self.timeouts.last().map(|(_, t)| {
            if *t >= now {
                *t - now
            } else {
                Duration::from_secs(0)
            }
        })
    }

    /// Given a specific time, add to the input vector keys that
    /// have timed out by that time. Returns the number of keys that timed out.
    pub fn check(&mut self, time: Instant, fired: &mut Vec<K>) -> usize {
        let before = fired.len();

        while let Some((k, t)) = self.timeouts.pop() {
            if time >= t {
                fired.push(k);
            } else {
                self.timeouts.push((k, t));
                break;
            }
        }
        fired.len() - before
    }

    /// Given the current time, add to the input vector keys that
    /// have timed out. Returns the number of keys that timed out.
    ///
    /// ```
    /// use std::time::{Duration, Instant};
    /// use reactor::managers::TimeoutManager;
    ///
    /// let mut tm = TimeoutManager::new(Duration::from_secs(0));
    /// let now = Instant::now();
    ///
    /// tm.register(0xA, now + Duration::from_millis(8));
    /// tm.register(0xB, now + Duration::from_millis(16));
    /// tm.register(0xC, now + Duration::from_millis(64));
    /// tm.register(0xD, now + Duration::from_millis(72));
    ///
    /// let mut timeouts = Vec::new();
    ///
    /// assert_eq!(tm.check(now + Duration::from_millis(21), &mut timeouts), 2);
    /// assert_eq!(timeouts, vec![0xA, 0xB]);
    /// assert_eq!(tm.len(), 2);
    /// ```
    pub fn check_now(&mut self, fired: &mut Vec<K>) -> usize {
        self.check(Instant::now(), fired)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wake() {
        let mut tm = TimeoutManager::new(Duration::from_secs(0));
        let now = Instant::now();

        tm.register(0xA, now + Duration::from_millis(8));
        tm.register(0xB, now + Duration::from_millis(16));
        tm.register(0xC, now + Duration::from_millis(64));
        tm.register(0xD, now + Duration::from_millis(72));

        let mut timeouts = Vec::new();

        assert_eq!(tm.check(now, &mut timeouts), 0);
        assert_eq!(timeouts, vec![]);
        assert_eq!(tm.len(), 4);
        assert_eq!(tm.check(now + Duration::from_millis(9), &mut timeouts), 1);
        assert_eq!(timeouts, vec![0xA]);
        assert_eq!(tm.len(), 3, "one timeout has expired");

        timeouts.clear();

        assert_eq!(tm.check(now + Duration::from_millis(66), &mut timeouts), 2);
        assert_eq!(timeouts, vec![0xB, 0xC]);
        assert_eq!(tm.len(), 1, "another two timeouts have expired");

        timeouts.clear();

        assert_eq!(tm.check(now + Duration::from_millis(96), &mut timeouts), 1);
        assert_eq!(timeouts, vec![0xD]);
        assert!(tm.is_empty(), "all timeouts have expired");
    }
}
