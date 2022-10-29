use std::collections::{HashMap, VecDeque};
use std::hash::Hash;
use std::time::{Duration, Instant};
use std::{io, net};
use streampipes::{NetStream, OnDemand, Resource, ResourceAddr};

use super::TimeoutManager;
use crate::resources::tcp_raw::TcpSocket;
use crate::resources::{FdResource, TcpLocator};
use crate::{ConnDirection, InputEvent, ResourceMgr};

/// Maximum amount of time to wait for i/o.
const WAIT_TIMEOUT: Duration = Duration::from_secs(60 * 60);

/// Manager for a set of resources which are polled for an event loop by the
/// reactor by using [`popol`] library.
///
/// Able to perform handshake protocol on resource connection, upgrading the
/// resource address from [`R::Addr::Raw`] to [`R::Addr`].
///
/// Does not perform buffered write operations for the underlying resources
/// (see [`Self::send`] for the explanations). If non-blocking buffering must
/// be performed than a dedicated [`Resource`] type should be used which handles
/// the buffering internally.
pub struct PollManager<R>
where
    R: FdResource,
    R::Addr: Hash,
{
    poll: popol::Poll<R::Addr>,
    connecting: HashMap<<R::Addr as ResourceAddr>::Raw, R::Raw>,
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
            mgr.connect_resource(&TcpLocator::Connection(addr))?;
        }

        Ok(mgr)
    }
}

impl<R> PollManager<R>
where
    R: FdResource,
    R::Addr: Hash,
{
    fn register_raw(&mut self, raw: R::Raw, addr: Option<R::Addr>) {
        // TODO: Add to poll if an address is present
        todo!()
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
    type EventIterator = VecDeque<InputEvent<R>>;

    fn get(&self, addr: &R::Addr) -> Option<&R> {
        self.resources.get(addr)
    }

    // Called for outcoming connections
    fn connect_resource(&mut self, addr: &R::Addr) -> Result<bool, R::Error> {
        if self.has(addr) || self.connecting.contains_key(&addr.to_raw()) {
            return Ok(false);
        }
        let raw = R::raw_connection(addr)?;
        self.register_raw(raw, Some(addr.to_owned()));
        Ok(true)
    }

    fn disconnect_resource(&mut self, addr: &R::Addr) -> Result<bool, R::Error> {
        if !self.has(addr) || self.connecting.contains_key(&addr.to_raw()) {
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

            self.events.reserve(events.len());
            for event in events {
                match &event {
                    InputEvent::Spawn(raw) => {}
                    InputEvent::Upgraded() => {
                        // TODO: Move resource to the upgraded protocol
                    }
                    InputEvent::Connected { .. } => {}
                    InputEvent::Disconnected(addr, _) => {
                        debug_assert!(
                            // We do not use `self.unregister_resource` here due to borrower checker problem
                            self.resources.remove(addr).is_some(),
                            "broken connection management"
                        );
                    }
                    InputEvent::Timeout => {}
                    InputEvent::Timer => {}
                    InputEvent::Received(remote_addr, _) => {
                        if self.connecting.remove(&remote_addr.to_raw()).is_some() {
                            self.events.push_back(InputEvent::Connected {
                                remote_addr: remote_addr.to_owned(),
                                direction: ConnDirection::Outbound,
                            });
                        }
                        // TODO: check raw list and generate inbound connection event
                        // remove from connecting
                    }
                }
                self.events.push_back(event);
            }
        }

        Ok(&mut self.events)
    }

    fn events(&mut self) -> &mut Self::EventIterator {
        &mut self.events
    }

    fn into_events(self) -> Self::EventIterator {
        self.events
    }

    fn split_events(mut self) -> (Self, Self::EventIterator) {
        let events = self.events;
        self.events = VecDeque::with_capacity(events.len());
        (self, events)
    }

    fn join_events(&mut self, events: Self::EventIterator) {
        self.events.extend(events)
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
