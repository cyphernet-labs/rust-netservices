//! Noise_XK streams, connections and sessions based on TCP stream

use std::io::{self, Read, Write};
use std::net;
use std::net::{SocketAddr, TcpStream};
use std::os::unix::io::{AsFd, AsRawFd, BorrowedFd, RawFd};

use cyphernet::addr::{LocalNode, PeerAddr, UniversalAddr};
use cyphernet::crypto::ed25519::Curve25519;
use cyphernet::crypto::Ec;
use reactor::actors::stdtcp::{TcpAction, TcpConnection, TcpSpawner};
use reactor::actors::IoEv;
use reactor::{Actor, Controller, Layout};

use crate::noise_xk;

pub type NxkAddr<EC = Curve25519> = UniversalAddr<PeerAddr<EC, net::SocketAddr>>;

pub enum NxkAction<EC: Ec> {
    Accept(TcpStream, SocketAddr),
    Connect(NxkAddr<EC>),
}

impl<EC: Ec> From<NxkAction<EC>> for TcpAction {
    fn from(nxk: NxkAction<EC>) -> Self {
        match nxk {
            NxkAction::Accept(stream, addr) => TcpAction::Accept(stream, addr),
            NxkAction::Connect(nxk_addr) => TcpAction::Connect(nxk_addr.into()),
        }
    }
}

pub struct NxkContext<EC: Ec> {
    pub action: NxkAction<EC>,
    pub local_node: LocalNode<EC>,
}

pub type NxkStream<L, EC> = noise_xk::Stream<EC, TcpConnection<L>>;

pub struct NxkSession<L: Layout, EC: Ec = Curve25519> {
    stream: NxkStream<L, EC>,
    peer_addr: Option<NxkAddr<EC>>,
}

impl<L: Layout, EC: Ec> NxkSession<L, EC> {
    pub fn accept(
        tcp_stream: TcpStream,
        local_node: LocalNode<EC>,
        controller: Controller<L>,
    ) -> io::Result<Self> {
        let connection = TcpConnection::accept(tcp_stream, controller)?;
        let stream = NxkStream::upgrade(connection, local_node);
        Ok(Self {
            stream,
            peer_addr: None,
        })
    }
}

impl<L: Layout, EC: Ec> NxkSession<L, EC> {
    pub fn connect(
        nsh_addr: NxkAddr<EC>,
        local_node: LocalNode<EC>,
        controller: Controller<L>,
    ) -> io::Result<Self> {
        let remote_key = nsh_addr.as_remote_addr().to_pubkey();
        let connection = TcpConnection::connect(nsh_addr, controller)?;
        let stream = NxkStream::connect(connection, remote_key, local_node)?;
        Ok(Self {
            stream,
            peer_addr: Some(nsh_addr),
        })
    }
}

impl<L: Layout, EC: Ec> Actor for NxkSession<L, EC>
where
    EC: Copy,
    EC::PubKey: Send,
    EC::PrivKey: Send,
{
    type Layout = L;
    type Id = RawFd;
    type Context = NxkContext<EC>;
    type Cmd = Vec<u8>;
    type Error = io::Error;

    fn with(context: Self::Context, controller: Controller<L>) -> Result<Self, Self::Error>
    where
        Self: Sized,
    {
        match context.action {
            NxkAction::Accept(tcp_stream, _) => {
                Self::accept(tcp_stream, context.local_node, controller)
            }
            NxkAction::Connect(addr) => Self::connect(addr, context.local_node, controller),
        }
    }

    fn id(&self) -> Self::Id {
        self.stream.as_raw_fd()
    }

    fn io_ready(&mut self, io: IoEv) -> Result<(), Self::Error> {
        self.stream.as_inner_mut().io_ready(io)
    }

    fn handle_cmd(&mut self, data: Vec<u8>) -> Result<(), Self::Error> {
        self.stream.write_all(&data)
    }

    fn handle_err(&mut self, err: Self::Error) -> Result<(), Self::Error> {
        Err(err)
    }
}

impl<L: Layout, EC: Ec> Read for NxkSession<L, EC> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.stream.read(buf)
    }
}

impl<L: Layout, EC: Ec> Write for NxkSession<L, EC> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.stream.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.stream.flush()
    }
}

impl<L: Layout, EC: Ec> AsRawFd for NxkSession<L, EC> {
    fn as_raw_fd(&self) -> RawFd {
        self.stream.as_raw_fd()
    }
}

impl<L: Layout, EC: Ec> AsFd for NxkSession<L, EC> {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.stream.as_fd()
    }
}

pub struct NxkSpawner<L: Layout, const SESSION_POOL_ID: u32, EC: Ec = Curve25519> {
    socket: TcpSpawner<L, SESSION_POOL_ID>,
    local_node: LocalNode<EC>,
}

impl<L: Layout, const SESSION_POOL_ID: u32, EC> Actor for NxkSpawner<L, SESSION_POOL_ID, EC>
where
    EC: Ec + Clone + 'static,
    EC::PubKey: Send + Clone,
    EC::PrivKey: Send + Clone,
{
    type Layout = L;
    type Id = RawFd;
    type Context = (LocalNode<EC>, net::SocketAddr);
    type Cmd = ();
    type Error = io::Error;

    fn with(context: Self::Context, controller: Controller<L>) -> Result<Self, Self::Error>
    where
        Self: Sized,
    {
        let socket = TcpSpawner::with(context.1, controller)?;
        Ok(Self {
            socket,
            local_node: context.0,
        })
    }

    fn id(&self) -> Self::Id {
        self.socket.as_raw_fd()
    }

    fn io_ready(&mut self, io: IoEv) -> Result<(), Self::Error> {
        self.socket.io_ready(io)
    }

    fn handle_cmd(&mut self, cmd: Self::Cmd) -> Result<(), Self::Error> {
        self.socket.handle_cmd(cmd)
    }

    fn handle_err(&mut self, err: Self::Error) -> Result<(), Self::Error> {
        self.socket.handle_err(err)
    }
}

impl<L: Layout, const SESSION_POOL_ID: u32, EC: Ec> AsRawFd for NxkSpawner<L, SESSION_POOL_ID, EC> {
    fn as_raw_fd(&self) -> RawFd {
        self.socket.as_raw_fd()
    }
}

impl<L: Layout, const SESSION_POOL_ID: u32, EC: Ec> AsFd for NxkSpawner<L, SESSION_POOL_ID, EC> {
    fn as_fd(&self) -> BorrowedFd<'_> {
        self.socket.as_fd()
    }
}
