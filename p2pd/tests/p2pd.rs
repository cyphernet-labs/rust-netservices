use std::error::Error as StdError;
use std::os::fd::{AsRawFd, RawFd};
use std::{io, net};

use cyphernet::addr::LocalNode;
use cyphernet::crypto::ed25519::{Curve25519, PrivateKey};
use ioreactor::popol::PopolScheduler;
use ioreactor::{Actor, Controller, Handler, IoEv, Reactor, ReactorApi};
use p2pd::nxk_tcp::{NxkAction, NxkAddr, NxkContext, NxkListener, NxkSession};

fn main() -> Result<(), Box<dyn StdError>> {
    let manager = PopolScheduler::<Actor>::new();
    let broker = Service {};
    let mut reactor = Reactor::new(manager, broker)?;

    let nsh_socket = Context {
        method: Action::Connect("127.0.0.1".parse().unwrap()),
        local_node: LocalNode::from(PrivateKey::test()),
    };
    reactor.start_actor(nsh_socket)?;
    reactor.join().unwrap();
    Ok(())
}

pub enum Action {
    Listen(net::SocketAddr),
    Accept(net::TcpStream, net::SocketAddr),
    Connect(NxkAddr),
}

impl From<NxkAction<Curve25519>> for Action {
    fn from(method: NxkAction<Curve25519>) -> Self {
        match method {
            NxkAction::Accept(stream, addr) => Self::Accept(stream, addr),
            NxkAction::Connect(addr) => Self::Connect(addr),
        }
    }
}

pub struct Context {
    pub method: Action,
    pub local_node: LocalNode<Curve25519>,
}

impl From<NxkContext<Curve25519>> for Context {
    fn from(ctx: NxkContext<Curve25519>) -> Self {
        Context {
            method: ctx.method.into(),
            local_node: ctx.local_node,
        }
    }
}

pub enum Actor {
    Listener(NxkListener<Self>),
    Session(NxkSession),
}

impl Actor for Actor {
    type Id = RawFd;
    type Context = Context;
    type Cmd = Vec<u8>;
    type Error = io::Error;

    fn with(context: Self::Context, controller: Controller<Self>) -> Result<Self, Self::Error>
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

pub struct Service {}

impl Handler<Actor> for Service {
    fn handle_err(&mut self, err: io::Error) {
        panic!("{}", err);
    }
}
