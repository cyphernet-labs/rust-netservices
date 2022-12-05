use std::io::{self, Read, Write};
use std::net::Shutdown;
use std::os::unix::io::{AsRawFd, RawFd};
use std::time::Duration;

use cyphernet::addr::{LocalNode, NodeId, PeerAddr};
use cyphernet::crypto::Ec;

use crate::resources::socket::NetSession;
use crate::stream::NetStream;

pub struct NoiseXk<C: Ec, S: NetStream> {
    remote_addr: PeerAddr<NodeId<C>, S::Addr>,
    local_node: LocalNode<C>,
    socket: S,
}

impl<C: Ec, S: NetStream> AsRawFd for NoiseXk<C, S> {
    fn as_raw_fd(&self) -> RawFd {
        self.socket.as_raw_fd()
    }
}

impl<C: Ec, S: NetStream> Read for NoiseXk<C, S> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.socket.read(buf)
    }
}

impl<C: Ec, S: NetStream> Write for NoiseXk<C, S> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.socket.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.socket.flush()
    }
}

impl<C: Ec, S: NetStream> NetSession for NoiseXk<C, S> {
    type Context = (NodeId<C>, LocalNode<C>);
    type Inner = S;

    fn with(socket: S, context: &Self::Context) -> Self {
        let remote_addr = PeerAddr::new(
            context.0,
            socket
                .peer_addr()
                .expect("constructing NoiseXK with net session object not having remote address"),
        );
        Self {
            remote_addr,
            local_node: context.1.clone(),
            socket,
        }
    }

    fn remote_addr(&self) -> Self::Addr {
        self.remote_addr.clone()
    }
}

impl<C: Ec, S: NetStream> NetStream for NoiseXk<C, S> {
    type Addr = PeerAddr<NodeId<C>, S::Addr>;

    fn shutdown(&mut self, how: Shutdown) -> io::Result<()> {
        self.socket.shutdown(how)
    }

    fn peer_addr(&self) -> io::Result<Self::Addr> {
        Ok(self.remote_addr.clone())
    }

    fn local_addr(&self) -> io::Result<Self::Addr> {
        let local_addr = self.socket.local_addr()?;
        Ok(PeerAddr::new(self.local_node.id(), local_addr))
    }

    fn set_read_timeout(&mut self, dur: Option<Duration>) -> io::Result<()> {
        self.socket.set_read_timeout(dur)
    }

    fn set_write_timeout(&mut self, dur: Option<Duration>) -> io::Result<()> {
        self.socket.set_write_timeout(dur)
    }

    fn read_timeout(&self) -> io::Result<Option<Duration>> {
        self.socket.read_timeout()
    }

    fn write_timeout(&self) -> io::Result<Option<Duration>> {
        self.socket.write_timeout()
    }

    fn peek(&self, buf: &mut [u8]) -> io::Result<usize> {
        self.socket.peek(buf)
    }

    fn set_nodelay(&mut self, nodelay: bool) -> io::Result<()> {
        self.socket.set_nodelay(nodelay)
    }

    fn nodelay(&self) -> io::Result<bool> {
        self.socket.nodelay()
    }

    fn set_ttl(&mut self, ttl: u32) -> io::Result<()> {
        self.socket.set_ttl(ttl)
    }

    fn ttl(&self) -> io::Result<u32> {
        self.socket.ttl()
    }

    fn set_nonblocking(&mut self, nonblocking: bool) -> io::Result<()> {
        self.socket.set_nonblocking(nonblocking)
    }

    fn try_clone(&self) -> io::Result<Self> {
        Ok(Self {
            remote_addr: self.remote_addr.clone(),
            local_node: self.local_node.clone(),
            socket: self.socket.try_clone()?,
        })
    }

    fn take_error(&self) -> io::Result<Option<io::Error>> {
        self.socket.take_error()
    }
}
