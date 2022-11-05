//! Noise_XK streams (can be any)
// TODO: Wrap actual noise_xk functionality
use std::io::{self, Read, Write};
use std::os::unix::io::{AsRawFd, RawFd};

use cyphernet::addr::LocalNode;
use cyphernet::crypto::Ec;
use streampipes::NetStream;

pub struct Stream<EC: Ec, S: streampipes::Stream> {
    stream: S,
    local_node: LocalNode<EC>,
    remote_key: Option<EC::PubKey>,
}

impl<EC: Ec, S: streampipes::Stream> Stream<EC, S> {
    pub fn connect(
        stream: S,
        remote_key: EC::PubKey,
        local_node: LocalNode<EC>,
    ) -> io::Result<Self> {
        Ok(Self {
            stream,
            local_node,
            remote_key: Some(remote_key),
        })
    }

    pub fn upgrade(stream: S, local_node: LocalNode<EC>) -> Self {
        Self {
            stream,
            local_node,
            remote_key: None,
        }
    }
}

impl<EC: Ec, S: streampipes::Stream> Read for Stream<EC, S> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.stream.read(buf)
    }
}

impl<EC: Ec, S: streampipes::Stream> Write for Stream<EC, S> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.stream.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.stream.flush()
    }
}

impl<EC, S> AsRawFd for Stream<EC, S>
where
    EC: Ec,
    S: NetStream,
{
    fn as_raw_fd(&self) -> RawFd {
        self.stream.as_raw_fd()
    }
}
