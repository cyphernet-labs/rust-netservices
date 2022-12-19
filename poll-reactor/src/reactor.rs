use std::collections::HashMap;
use std::io;
use std::io::Write;
use std::os::unix::io::{AsRawFd, RawFd};
use std::os::unix::net::UnixStream;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use crossbeam_channel as chan;

use crate::poller::Poll;
use crate::{Resource, ResourceId};

#[derive(Debug, Display, Error)]
#[display(doc_comments)]
pub enum Error<L: ResourceId, T: ResourceId> {
    /// unknown listener {0}
    ListenerUnknown(L),

    /// no connection with to peer {0}
    PeerUnknown(T),

    /// connection with peer {0} got broken
    PeerDisconnected(T, io::Error),
}

pub enum Action<L: Resource, T: Resource> {
    RegisterListener(L),
    RegisterTransport(T),
    UnregisterListener(L::Id),
    UnregisterTransport(T::Id),
    Send(T::Id, Vec<T::Message>),
    SetTimer(Duration),
}

pub trait Handler: Send + Iterator<Item = Action<Self::Listener, Self::Transport>> {
    type Listener: Resource;
    type Transport: Resource;
    type Command: Send;

    fn handle_wakeup(&mut self);

    fn handle_listener_event(
        &mut self,
        id: <Self::Listener as Resource>::Id,
        event: <Self::Listener as Resource>::Event,
        duration: Duration,
    );

    fn handle_transport_event(
        &mut self,
        id: <Self::Transport as Resource>::Id,
        event: <Self::Transport as Resource>::Event,
        duration: Duration,
    );

    fn handle_command(&mut self, cmd: Self::Command);

    fn handle_error(
        &mut self,
        err: Error<<Self::Listener as Resource>::Id, <Self::Transport as Resource>::Id>,
    );

    /// Called by the reactor upon receiving [`Action::UnregisterListener`]
    fn handover_listener(&mut self, listener: Self::Listener);
    /// Called by the reactor upon receiving [`Action::UnregisterTransport`]
    fn handover_transport(&mut self, transport: Self::Transport);
}

pub struct Reactor<S: Handler> {
    thread: JoinHandle<()>,
    controller: Controller<S::Command>,
}

impl<S: Handler> Reactor<S> {
    pub fn new<P: Poll>(service: S, mut poller: P) -> Result<Self, io::Error>
    where
        S: 'static,
        P: 'static,
    {
        let (shutdown_send, shutdown_recv) = chan::bounded(1);
        let (control_send, control_recv) = chan::unbounded();

        let (waker_writer, waker_reader) = UnixStream::pair()?;
        waker_reader.set_nonblocking(true)?;
        waker_writer.set_nonblocking(true)?;

        let thread = std::thread::spawn(move || {
            poller.register(waker_reader.as_raw_fd());

            let runtime = Runtime {
                service,
                poller,
                control_recv,
                shutdown_recv,
                listeners: empty!(),
                transports: empty!(),
                listener_map: empty!(),
                transport_map: empty!(),
                waker: waker_reader,
            };

            runtime.run();
        });

        let controller = Controller {
            control_send,
            shutdown_send,
            waker: Arc::new(Mutex::new(waker_writer)),
        };
        Ok(Self { thread, controller })
    }

    pub fn controller(&self) -> Controller<S::Command> {
        self.controller.clone()
    }

    pub fn join(self) -> std::thread::Result<()> {
        self.thread.join()
    }
}

pub struct Controller<C> {
    control_send: chan::Sender<C>,
    shutdown_send: chan::Sender<()>,
    waker: Arc<Mutex<UnixStream>>,
}

impl<C> Clone for Controller<C> {
    fn clone(&self) -> Self {
        Controller {
            control_send: self.control_send.clone(),
            shutdown_send: self.shutdown_send.clone(),
            waker: self.waker.clone(),
        }
    }
}

impl<C> Controller<C> {
    pub fn send(&self, command: C) -> Result<(), io::Error> {
        self.control_send
            .send(command)
            .map_err(|_| io::ErrorKind::BrokenPipe)?;
        self.wake()?;
        Ok(())
    }

    pub fn shutdown(self) -> Result<(), Self> {
        let res = self.shutdown_send.send(());
        self.wake().expect("waker socket failure");
        res.map_err(|_| self)
    }

    fn wake(&self) -> io::Result<()> {
        use io::ErrorKind::*;

        let mut waker = self.waker.lock().map_err(|_| io::ErrorKind::WouldBlock)?;
        match waker.write_all(&[0x1]) {
            Ok(_) => Ok(()),
            Err(e) if e.kind() == WouldBlock => {
                reset_fd(&waker.as_raw_fd())?;
                self.wake()
            }
            Err(e) if e.kind() == Interrupted => self.wake(),
            Err(e) => Err(e),
        }
    }
}

fn reset_fd(fd: &impl AsRawFd) -> io::Result<()> {
    let mut buf = [0u8; 4096];

    loop {
        // We use a low-level "read" here because the alternative is to create a `UnixStream`
        // from the `RawFd`, which has "drop" semantics which we want to avoid.
        match unsafe {
            libc::read(
                fd.as_raw_fd(),
                buf.as_mut_ptr() as *mut libc::c_void,
                buf.len(),
            )
        } {
            -1 => match io::Error::last_os_error() {
                e if e.kind() == io::ErrorKind::WouldBlock => return Ok(()),
                e => return Err(e),
            },
            0 => return Ok(()),
            _ => continue,
        }
    }
}

pub struct Runtime<H: Handler, P: Poll> {
    service: H,
    poller: P,
    control_recv: chan::Receiver<H::Command>,
    shutdown_recv: chan::Receiver<()>,
    listener_map: HashMap<RawFd, <H::Listener as Resource>::Id>,
    transport_map: HashMap<RawFd, <H::Transport as Resource>::Id>,
    listeners: HashMap<<H::Listener as Resource>::Id, H::Listener>,
    transports: HashMap<<H::Transport as Resource>::Id, H::Transport>,
    waker: UnixStream,
    // timeouts
}

impl<H: Handler, P: Poll> Runtime<H, P> {
    fn run(mut self) {
        loop {
            let (duration, count) = self.poller.poll();
            if count > 0 {
                self.handle_events(duration);
            }
            // TODO process commands
        }
    }

    fn handle_events(&mut self, duration: Duration) {
        for (fd, io) in &mut self.poller {
            if fd == self.waker.as_raw_fd() {
                reset_fd(&self.waker).expect("waiker failure")
            } else if let Some(id) = self.listener_map.get(&fd) {
                let res = self.listeners.get_mut(id).expect("resource disappeared");
                res.handle_io(io);
                for event in res {
                    self.service.handle_listener_event(*id, event, duration);
                }
            } else if let Some(id) = self.transport_map.get(&fd) {
                let res = self.transports.get_mut(id).expect("resource disappeared");
                res.handle_io(io);
                for event in res {
                    self.service.handle_transport_event(*id, event, duration);
                }
            }
        }

        while let Some(action) = self.service.next() {
            // NB: Deadlock may happen here if the service will generate events over and over
            // in the handle_* calls we may never get out of this loop
            if let Err(err) = self.handle_action(action) {
                self.service.handle_error(err);
            }
        }
    }

    fn handle_action(
        &mut self,
        action: Action<H::Listener, H::Transport>,
    ) -> Result<(), Error<<H::Listener as Resource>::Id, <H::Transport as Resource>::Id>> {
        match action {
            Action::RegisterListener(listener) => {
                let id = listener.id();
                let fd = listener.as_raw_fd();
                self.poller.register(fd);
                self.listeners.insert(id, listener);
                self.listener_map.insert(fd, id);
            }
            Action::RegisterTransport(transport) => {
                let id = transport.id();
                let fd = transport.as_raw_fd();
                self.poller.register(fd);
                self.transports.insert(id, transport);
                self.transport_map.insert(fd, id);
            }
            Action::UnregisterListener(id) => {
                let listener = self
                    .listeners
                    .remove(&id)
                    .ok_or(Error::ListenerUnknown(id))?;
                let fd = listener.as_raw_fd();
                self.listener_map
                    .remove(&fd)
                    .expect("listener index content doesn't match registered listeners");
                self.poller.unregister(fd);
                self.service.handover_listener(listener);
            }
            Action::UnregisterTransport(id) => {
                let transport = self.transports.remove(&id).ok_or(Error::PeerUnknown(id))?;
                let fd = transport.as_raw_fd();
                self.transport_map
                    .remove(&fd)
                    .expect("transport index content doesn't match registered transports");
                self.poller.unregister(fd);
                self.service.handover_transport(transport);
            }
            Action::Send(id, msgs) => {
                let transport = self.transports.get_mut(&id).ok_or(Error::PeerUnknown(id))?;
                for msg in msgs {
                    // If we fail on sending any message this means disconnection (I/O write
                    // has failed for a given transport). We report error -- and lose all other
                    // messages we planned to send
                    transport
                        .send(msg)
                        .map_err(|err| Error::PeerDisconnected(id, err))?;
                }
            }
            Action::SetTimer(duration) => {
                // TODO: Set timer
            }
        }
        Ok(())
    }
}
