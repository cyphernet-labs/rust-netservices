use std::io::Write;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::os::unix::io::{AsRawFd, RawFd};
use std::{io, net};

use crate::actors::IoEv;
use crate::{Actor, Controller, Layout, ReactorApi};

pub enum TcpAction {
    Accept(TcpStream, SocketAddr),
    Connect(SocketAddr),
}

pub struct TcpConnection<L: Layout> {
    stream: TcpStream,
    inbound: bool,
    controller: Controller<L>,
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
        let (inbound, stream) = match context {
            TcpAction::Accept(stream, _addr) => (true, stream),
            TcpAction::Connect(addr) => (false, TcpStream::connect(addr)?),
        };
        Ok(Self {
            stream,
            controller,
            inbound,
        })
    }

    fn id(&self) -> Self::Id {
        self.stream.as_raw_fd()
    }

    fn io_ready(&mut self, io: IoEv) -> Result<(), Self::Error> {
        // We do nothing - data must be handler in the upper level resource
        Ok(())
    }

    fn handle_cmd(&mut self, data: Self::Cmd) -> Result<(), Self::Error> {
        self.stream.write_all(&data)
    }

    fn handle_err(&mut self, err: Self::Error) -> Result<(), Self::Error> {
        Err(err)
    }
}

pub struct TcpSpawner<L: Layout, const SESSION_POOL_ID: u32> {
    socket: TcpListener,
    controller: Controller<L>,
}

impl<L: Layout, const SESSION_POOL_ID: u32> AsRawFd for TcpSpawner<L, SESSION_POOL_ID> {
    fn as_raw_fd(&self) -> RawFd {
        self.socket.as_raw_fd()
    }
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
        let socket = net::TcpListener::bind(context)?;
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
