use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::Hash;
use std::time::{Duration, Instant};
use std::{io, net};

use super::TimeoutManager;
use crate::resources::tcp::TcpSocket;
use crate::resources::{FdResource, TcpLocator};
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
            let key = TcpLocator::Listener(addr);
            poll.register(key, &socket, popol::event::ALL);
        }

        for addr in connect.to_socket_addrs()? {
            let socket = TcpSocket::dial(addr)?;
            let key = TcpLocator::Connection(addr);
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
                src.handle_writable(&mut events)?;
            }
            if ev.is_readable() {
                src.handle_readable(&mut events)?;
            }

            for event in &events {
                if let InputEvent::Connected { remote_addr, .. } = event {
                    self.connecting.remove(remote_addr);
                }
                if let InputEvent::Disconnected(addr, ..) = event {
                    debug_assert!(
                        // We do not use `self.unregister_resource` here due to borrower checker problem
                        self.resources.remove(addr).is_some(),
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

    // Write::write_all for the R must be a non-blocking call which uses internal OS buffer.
    // (for instance, this is the case for TcpStream and is stated in the io::Write docs).
    // The actual block happens in Write::flush which is called from R::handle_writable.
    //
    // If the underlying resource does not provide buffered write than it should be nested
    // into a buffered stream type and then provided to the manager as the resource.
    fn send(&mut self, addr: &R::Addr, data: impl AsRef<[u8]>) -> Result<usize, R::Error> {
        self.resources
            .get_mut(addr)
            .expect("broken resource index")
            .write_all(data.as_ref())?;
        /*
        self.write_queue
            .entry(addr)
            .or_default()
            .push_back(data.as_ref().into());
         */
        Ok(data.as_ref().len())
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
