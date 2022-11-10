//! Noise_XK streams (can be any)
// TODO: Wrap actual noise_xk functionality
use std::io::{self, Read, Write};
use std::os::unix::io::{AsFd, AsRawFd, BorrowedFd, RawFd};

use cyphernet::addr::LocalNode;
use cyphernet::crypto::Ec;

pub struct Stream<EC: Ec, S: streampipes::Stream> {
    inner: S,
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
            inner: stream,
            local_node,
            remote_key: Some(remote_key),
        })
    }

    pub fn upgrade(stream: S, local_node: LocalNode<EC>) -> Self {
        Self {
            inner: stream,
            local_node,
            remote_key: None,
        }
    }

    pub fn as_inner(&self) -> &S {
        &self.inner
    }

    pub fn as_inner_mut(&mut self) -> &mut S {
        &mut self.inner
    }
}

impl<EC: Ec, S: streampipes::Stream> Read for Stream<EC, S> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.inner.read(buf)
    }
}

impl<EC: Ec, S: streampipes::Stream> Write for Stream<EC, S> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.inner.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

impl<EC, S> AsRawFd for Stream<EC, S>
where
    EC: Ec,
    S: AsRawFd + streampipes::Stream,
{
    fn as_raw_fd(&self) -> RawFd {
        self.inner.as_raw_fd()
    }
}

impl<EC, S> AsFd for Stream<EC, S>
where
    EC: Ec,
    S: AsFd + streampipes::Stream,
{
    fn as_fd(&self) -> BorrowedFd {
        self.inner.as_fd()
    }
}
