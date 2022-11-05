use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
use std::os::unix::io::{AsFd, AsRawFd, BorrowedFd, RawFd};

use crate::actors::IoEv;
use crate::{Actor, Controller, Layout, ReactorApi};

pub enum TcpAction {
    Accept(TcpStream, SocketAddr),
    Connect(SocketAddr),
}

pub struct TcpConnection<L: Layout> {
    stream: TcpStream,
    queue: VecDeque<u8>,
    read_buf: Vec<u8>,
    pub(super) controller: Controller<L>,
    pub(super) is_inbound: bool,
}

impl<L: Layout> TcpConnection<L> {
    pub fn connect(addr: impl ToSocketAddrs, controller: Controller<L>) -> io::Result<Self> {
        let read_buf = Vec::with_capacity(u16::MAX as usize);

        Ok(Self {
            stream: TcpStream::connect(addr)?,
            queue: empty!(),
            read_buf,
            controller,
            is_inbound: false,
        })
    }

    pub fn accept(stream: TcpStream, controller: Controller<L>) -> io::Result<Self> {
        let read_buf = Vec::with_capacity(u16::MAX as usize);

        Ok(Self {
            stream,
            queue: empty!(),
            read_buf,
            controller,
            is_inbound: true,
        })
    }
}

impl<L: Layout> Actor for TcpConnection<L> {
    type Layout = L;
    type Id = RawFd;
    type Context = TcpAction;
    type Cmd = Vec<u8>;
    type Error = io::Error;

    fn with(
        context: Self::Context,
        controller: Controller<Self::Layout>,
    ) -> Result<Self, Self::Error>
    where
        Self: Sized,
    {
        match context {
            TcpAction::Accept(stream, _addr) => Self::accept(stream, controller),
            TcpAction::Connect(addr) => Self::connect(addr, controller),
        }
    }

    fn id(&self) -> Self::Id {
        self.stream.as_raw_fd()
    }

    fn io_ready(&mut self, io: IoEv) -> Result<(), Self::Error> {
        if io.is_readable {
            self.flush()?;
        }
        if io.is_writable {
            let len = self.stream.read(&mut self.read_buf)?;
            self.queue.extend(&self.read_buf[..len]);
        }
        Ok(())
    }

    fn handle_cmd(&mut self, data: Self::Cmd) -> Result<(), Self::Error> {
        self.write_all(&data)
    }

    fn handle_err(&mut self, err: Self::Error) -> Result<(), Self::Error> {
        Err(err)
    }
}

impl<L: Layout> Read for TcpConnection<L> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.queue.read(buf)
    }
}

impl<L: Layout> Write for TcpConnection<L> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.stream.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.stream.flush()
    }
}

impl<L: Layout> AsRawFd for TcpConnection<L> {
    fn as_raw_fd(&self) -> RawFd {
        self.stream.as_raw_fd()
    }
}

impl<L: Layout> AsFd for TcpConnection<L> {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.stream.as_fd()
    }
}

pub struct TcpSpawner<L: Layout, const SESSION_POOL_ID: u32> {
    socket: TcpListener,
    controller: Controller<L>,
}

impl<L: Layout, const SESSION_POOL_ID: u32> Actor for TcpSpawner<L, SESSION_POOL_ID> {
    type Layout = L;
    type Id = RawFd;
    type Context = SocketAddr;
    type Cmd = ();
    type Error = io::Error;

    fn with(context: Self::Context, controller: Controller<L>) -> Result<Self, Self::Error>
    where
        Self: Sized,
    {
        let socket = TcpListener::bind(context)?;
        socket.set_nonblocking(true)?;
        Ok(Self { socket, controller })
    }

    fn id(&self) -> Self::Id {
        self.socket.as_raw_fd()
    }

    fn io_ready(&mut self, _: IoEv) -> Result<(), Self::Error> {
        let (stream, peer_socket_addr) = self.socket.accept()?;
        let action = TcpAction::Accept(stream, peer_socket_addr);
        let ctx = L::convert(Box::new(action));
        self.controller
            .start_actor(SESSION_POOL_ID.into(), ctx)
            .map_err(|_| io::ErrorKind::NotConnected)?;
        Ok(())
    }

    fn handle_cmd(&mut self, _cmd: Self::Cmd) -> Result<(), Self::Error> {
        // Listener does not support any commands
        Ok(())
    }

    fn handle_err(&mut self, err: Self::Error) -> Result<(), Self::Error> {
        // Listener does not know how to handle errors, so it just propagates them
        Err(err)
    }
}

impl<L: Layout, const SESSION_POOL_ID: u32> AsRawFd for TcpSpawner<L, SESSION_POOL_ID> {
    fn as_raw_fd(&self) -> RawFd {
        self.socket.as_raw_fd()
    }
}

impl<L: Layout, const SESSION_POOL_ID: u32> AsFd for TcpSpawner<L, SESSION_POOL_ID> {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.socket.as_fd()
    }
}
