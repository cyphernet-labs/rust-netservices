use std::collections::{HashMap, HashSet, VecDeque};
use std::hash::Hash;
use std::time::{Duration, Instant};
use std::{io, net};
use streampipes::{NetStream, OnDemand, Resource, ResourceAddr};

use super::TimeoutManager;
use crate::resources::tcp::TcpSocket;
use crate::resources::{FdResource, TcpLocator};
use crate::{ConnDirection, InputEvent, ResourceMgr};

/// Maximum amount of time to wait for i/o.
const WAIT_TIMEOUT: Duration = Duration::from_secs(60 * 60);

/// Manager for a set of resources which are polled for an event loop by the
/// reactor by using [`popol`] library.
///
/// Able to perform handshake protocol on resource connection, upgrading the
/// resource address from [`R::Addr::Raw`] to [`R::Addr`]. For this purpose it
/// co-operates with the reactor runtume, which manages the handshake workflow.
/// TODO: Move this to Handler type and manage by the Runtime
///
/// Does not perform buffered write operations for the underlying resources
/// (see [`Self::send`] for the explanations). If non-blocking buffering must
/// be performed than a dedicated [`Resource`] type should be used which handles
/// the buffering internally.
pub struct PollManager<R: FdResource>
where
    R::Addr: Hash,
{
    poll: popol::Poll<R::Addr>,
    connecting: HashSet<<R::Addr as ResourceAddr>::Raw>,
    events: VecDeque<InputEvent<R>>,
    // We need this since [`popol::Poll`] keeps track of resources as of raw
    // file descriptors.
    resources: HashMap<R::Addr, R>,
    timeouts: TimeoutManager<()>,
}

impl<S> PollManager<TcpSocket<S>>
where
    S: NetStream + Send,
    S::Addr: ResourceAddr<Raw = net::SocketAddr> + Hash,
    TcpLocator<<S::Addr as ResourceAddr>::Raw>: From<TcpLocator<net::SocketAddr>>,
{
    pub fn new(
        listen: &impl net::ToSocketAddrs,
        connect: impl IntoIterator<Item = S::Addr>,
    ) -> io::Result<Self> {
        let mut poll = popol::Poll::new();

        for addr in listen.to_socket_addrs()? {
            let socket = TcpSocket::<S>::listen(addr)?;
            let key = TcpLocator::Listener(addr);
            poll.register(key, &socket, popol::event::ALL);
        }

        let timeouts = TimeoutManager::new(Duration::from_secs(1));
        let mut mgr = PollManager {
            poll,
            connecting: empty!(),
            events: empty!(),
            resources: empty!(),
            timeouts,
        };

        for addr in connect {
            mgr.connect_raw(&TcpLocator::Connection(addr), ConnDirection::Outbound)?;
        }

        Ok(mgr)
    }
}

impl<R> ResourceMgr<R> for PollManager<R>
where
    R: FdResource + Send,
    R::DisconnectReason: Send,
    R::Error: From<io::Error>,
    R::Raw: Send,
    R::Addr: Hash,
    <R::Addr as ResourceAddr>::Raw: Hash + From<<R::Raw as Resource>::Addr>,
{
    type EventIterator = Self;

    fn get(&self, addr: &R::Addr) -> Option<&R> {
        self.resources.get(addr)
    }

    fn is_connecting(&self, addr: &R::Addr) -> bool {
        self.connecting.contains(&addr.to_raw())
    }

    fn connect_resource(&mut self, resource: R) -> Option<&R> {
        let addr = resource.addr();
        if self.has(&addr) {
            return None;
        }
        Some(self.resources.entry(addr).or_insert(resource))
    }

    fn disconnect_resource(&mut self, addr: &R::Addr) -> Result<bool, R::Error> {
        if !self.has(addr) || self.connecting.contains(&addr.to_raw()) {
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

    fn _register_raw(&mut self, raw: R::Raw, direction: ConnDirection) -> bool {
        let raw_addr = raw.addr();
        if self.connecting.contains(&raw_addr) {
            return false;
        }
        self.events.push_back(InputEvent::RawConnected {
            remote_raw: raw,
            direction,
        });
        self.connecting.insert(raw_addr);
        true
    }

    fn register_resource(&mut self, resource: R) -> Option<&R> {
        let addr = resource.addr();
        self.poll
            .register(addr.clone(), &resource, popol::event::ALL);
        if self.resources.insert(addr.clone(), resource).is_some() {
            return None;
        } else {
            self.resources.get(&addr)
        }
    }

    fn unregister_resource(&mut self, addr: &R::Addr) -> Option<R> {
        self.poll.unregister(addr);
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
                if let InputEvent::RawConnected { remote_raw, .. } = event {
                    self.connecting.remove(&remote_raw.addr());
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

    /// Write::write_all for the R must be a non-blocking call which uses internal OS buffer.
    /// (for instance, this is the case for TcpStream and is stated in the io::Write docs).
    /// The actual block happens in Write::flush which is called from R::handle_writable.
    ///
    /// If the underlying resource does not provide buffered write than it should be nested
    /// into a buffered stream type and then provided to the manager as the resource.
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
    R: FdResource,
    R::Addr: Hash,
{
    type Item = InputEvent<R>;

    fn next(&mut self) -> Option<Self::Item> {
        self.events.pop_front()
    }
}
