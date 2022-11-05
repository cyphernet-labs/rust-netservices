use cyphernet::addr::LocalNode;
use cyphernet::crypto::ed25519::Curve25519;
use ioreactor::{Actor, Controller, IoEv, Pool};
use std::os::unix::io::{AsRawFd, RawFd};
use std::{io, net};

use crate::nxk_tcp::{NxkAction, NxkAddr, NxkContext, NxkListener, NxkSession};

pub enum Action {
    Listen(net::SocketAddr),
    Accept(net::TcpStream, net::SocketAddr),
    Connect(NxkAddr),
}

impl From<&NxkAction<Curve25519>> for Action {
    fn from(method: &NxkAction<Curve25519>) -> Self {
        match method {
            NxkAction::Accept(stream, addr) => Self::Accept(
                stream.try_clone().expect("TCP stream cloning failure"),
                *addr,
            ),
            NxkAction::Connect(addr) => Self::Connect(*addr),
        }
    }
}

pub struct Context {
    pub method: Action,
    pub local_node: LocalNode<Curve25519>,
}

impl From<&NxkContext<Curve25519>> for Context {
    fn from(ctx: &NxkContext<Curve25519>) -> Self {
        Context {
            method: (&ctx.action).into(),
            local_node: ctx.local_node.clone(),
        }
    }
}

pub enum PeerActor<P: Pool, const SESSION_POOL_ID: u32> {
    Listener(NxkListener<P, SESSION_POOL_ID>),
    Session(NxkSession<P>),
}

impl<P: Pool, const SESSION_POOL_ID: u32> Actor for PeerActor<P, SESSION_POOL_ID> {
    type Id = RawFd;
    type Context = Context;
    type Cmd = Vec<u8>;
    type Error = io::Error;
    type PoolSystem = P;

    fn with(
        context: Self::Context,
        controller: Controller<Self::PoolSystem>,
    ) -> Result<Self, Self::Error>
    where
        Self: Sized,
    {
        match context.method {
            Action::Listen(socket_addr) => {
                NxkListener::with((context.local_node, socket_addr), controller).map(Self::Listener)
            }
            Action::Accept(tcp_stream, remote_socket_addr) => Ok(Self::Session(
                NxkSession::accept(tcp_stream, remote_socket_addr, context.local_node),
            )),
            Action::Connect(nsh_addr) => {
                NxkSession::connect(nsh_addr, context.local_node).map(Self::Session)
            }
        }
    }

    fn id(&self) -> Self::Id {
        match self {
            Self::Listener(listener) => listener.as_raw_fd(),
            Self::Session(session) => session.as_raw_fd(),
        }
    }

    fn io_ready(&mut self, io: IoEv) -> Result<(), Self::Error> {
        match self {
            Self::Listener(listener) => listener.io_ready(io),
            Self::Session(session) => session.io_ready(io),
        }
    }

    fn handle_cmd(&mut self, cmd: Self::Cmd) -> Result<(), Self::Error> {
        match self {
            Self::Listener(_) => panic!("data sent to TCP listener"),
            Self::Session(stream) => stream.handle_cmd(cmd),
        }
    }

    fn handle_err(&mut self, err: Self::Error) -> Result<(), Self::Error> {
        log::error!("resource failure. Details: {}", err);
        Ok(())
    }
}
