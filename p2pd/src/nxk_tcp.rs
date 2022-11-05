//! Noise_XK streams, connections and sessions based on TCP stream

use std::io::{self, Read, Write};
use std::net;
use std::os::fd::{AsRawFd, RawFd};

use cyphernet::addr::{LocalNode, PeerAddr, UniversalAddr};
use cyphernet::crypto::ed25519::Curve25519;
use cyphernet::crypto::Ec;
use ioreactor::{Controller, IoEv, ReactorApi, Resource};

use crate::noise_xk;

pub type NxkAddr<EC = Curve25519> = UniversalAddr<PeerAddr<EC, net::SocketAddr>>;

pub enum NxkAction<EC: Ec> {
    Accept(net::TcpStream, net::SocketAddr),
    Connect(NxkAddr<EC>),
}

pub struct NxkContext<EC: Ec> {
    pub method: NxkAction<EC>,
    pub local_node: LocalNode<EC>,
}

pub type NxkStream<EC> = noise_xk::Stream<EC, net::TcpStream>;

pub struct NxkSession<EC: Ec = Curve25519> {
    stream: NxkStream<EC>,
    socket_addr: net::SocketAddr,
    peer_addr: Option<NxkAddr<EC>>,
    inbound: bool,
}

impl<EC: Ec> NxkSession<EC> {
    pub fn accept(
        tcp_stream: net::TcpStream,
        remote_socket_addr: net::SocketAddr,
        local_node: LocalNode<EC>,
    ) -> Self {
        Self {
            stream: NxkStream::upgrade(tcp_stream, local_node),
            socket_addr: remote_socket_addr,
            peer_addr: None,
            inbound: true,
        }
    }
}

impl<EC: Ec> NxkSession<EC>
where
    EC: Copy,
{
    pub fn connect(nsh_addr: NxkAddr<EC>, local_node: LocalNode<EC>) -> io::Result<Self> {
        // TODO: Use socks5
        let tcp_stream = net::TcpStream::connect(&nsh_addr)?;
        let remote_key = nsh_addr.as_remote_addr().to_pubkey();
        Ok(Self {
            stream: NxkStream::connect(tcp_stream, remote_key, local_node)?,
            socket_addr: nsh_addr.to_socket_addr(),
            peer_addr: Some(nsh_addr),
            inbound: false,
        })
    }
}

impl<EC: Ec> AsRawFd for NxkSession<EC> {
    fn as_raw_fd(&self) -> RawFd {
        self.stream.as_raw_fd()
    }
}

impl<EC: Ec> Read for NxkSession<EC> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.stream.read(buf)
    }
}

impl<EC: Ec> Write for NxkSession<EC> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.stream.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.stream.flush()
    }
}

impl<EC: Ec> Resource for NxkSession<EC>
where
    EC: Copy,
    EC::PubKey: Send,
    EC::PrivKey: Send,
{
    type Id = RawFd;
    type Context = NxkContext<EC>;
    type Cmd = Vec<u8>;
    type Error = io::Error;

    fn with(context: Self::Context, controller: Controller<Self>) -> Result<Self, Self::Error>
    where
        Self: Sized,
    {
        match context.method {
            NxkAction::Accept(tcp_stream, remote_socket_addr) => Ok(NxkSession::accept(
                tcp_stream,
                remote_socket_addr,
                context.local_node,
            )),
            NxkAction::Connect(nsh_addr) => NxkSession::connect(nsh_addr, context.local_node),
        }
    }

    fn id(&self) -> Self::Id {
        self.stream.as_raw_fd()
    }

    fn io_ready(&mut self, io: IoEv) -> Result<(), Self::Error> {
        if io.is_readable { /* TODO: Do read */ }
        if io.is_writable {
            self.flush()?;
        }
        Ok(())
    }

    fn handle_cmd(&mut self, data: Vec<u8>) -> Result<(), Self::Error> {
        self.stream.write_all(&data)
    }

    fn handle_err(&mut self, err: Self::Error) -> Result<(), Self::Error> {
        todo!()
    }
}

pub struct NxkListener<R, EC: Ec = Curve25519>
where
    R: Resource,
    R::Context: From<NxkContext<EC>>,
{
    socket: net::TcpListener,
    local_node: LocalNode<EC>,
    controller: Controller<R>,
}

impl<R, EC: Ec> AsRawFd for NxkListener<R, EC>
where
    R: Resource,
    R::Context: From<NxkContext<EC>>,
{
    fn as_raw_fd(&self) -> RawFd {
        self.socket.as_raw_fd()
    }
}

impl<R, EC> Resource<R> for NxkListener<R, EC>
where
    EC: Ec + Clone,
    EC::PubKey: Send + Clone,
    EC::PrivKey: Send + Clone,
    R: Resource,
    R::Context: From<NxkContext<EC>>,
{
    type Id = RawFd;
    type Context = (LocalNode<EC>, net::SocketAddr);
    type Cmd = ();
    type Error = io::Error;

    fn with(context: Self::Context, controller: Controller<R>) -> Result<Self, Self::Error>
    where
        Self: Sized,
    {
        let socket = net::TcpListener::bind(context.1)?;
        Ok(Self {
            socket,
            controller,
            local_node: context.0,
        })
    }

    fn id(&self) -> Self::Id {
        self.socket.as_raw_fd()
    }

    fn io_ready(&mut self, _: IoEv) -> Result<(), Self::Error> {
        let (stream, peer_socket_addr) = self.socket.accept()?;
        let nsh_info = NxkContext {
            method: NxkAction::Accept(stream, peer_socket_addr),
            local_node: self.local_node.clone(),
        };
        self.controller
            .connect(nsh_info.into())
            .map_err(|_| io::ErrorKind::NotConnected)?;
        Ok(())
    }

    fn handle_cmd(&mut self, cmd: Self::Cmd) -> Result<(), Self::Error> {
        // Listener does not support any commands
        Ok(())
    }

    fn handle_err(&mut self, err: Self::Error) -> Result<(), Self::Error> {
        // Listener does not know how to handle errors, so it just propagates them
        Err(err)
    }
}
