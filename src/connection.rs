// Library for building scalable privacy-preserving microservices P2P nodes
//
// SPDX-License-Identifier: Apache-2.0
//
// Written in 2022-2023 by
//     Dr. Maxim Orlovsky <orlovsky@cyphernet.org>
//
// Copyright 2022-2023 Cyphernet DAO, Switzerland
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

use std::fmt::{Debug, Display};
use std::hash::Hash;
use std::io;
use std::mem::MaybeUninit;
use std::net::{Shutdown, TcpStream, ToSocketAddrs};
use std::os::unix::io::AsRawFd;
use std::time::Duration;

use cyphernet::addr::{Addr, InetHost, NetAddr};

pub trait Address: Addr + Send + Clone + Eq + Hash + Debug + Display {}
impl<T> Address for T where T: Addr + Send + Clone + Eq + Hash + Debug + Display {}

pub trait NetStream: Send + io::Read + io::Write {}

/// Network stream is an abstraction of TCP stream object.
pub trait NetConnection: Send + NetStream + AsRawFd + Debug {
    type Addr: Address;

    fn connect_blocking(addr: Self::Addr) -> io::Result<Self>
    where
        Self: Sized;

    #[cfg(feature = "connect_nonblocking")]
    fn connect_nonblocking(addr: Self::Addr) -> io::Result<Self>
    where
        Self: Sized;

    fn shutdown(&mut self, how: Shutdown) -> io::Result<()>;

    fn remote_addr(&self) -> Self::Addr;
    fn local_addr(&self) -> Self::Addr;

    fn set_read_timeout(&mut self, dur: Option<Duration>) -> io::Result<()>;
    fn set_write_timeout(&mut self, dur: Option<Duration>) -> io::Result<()>;
    fn read_timeout(&self) -> io::Result<Option<Duration>>;
    fn write_timeout(&self) -> io::Result<Option<Duration>>;

    fn peek(&self, buf: &mut [u8]) -> io::Result<usize>;

    fn set_nodelay(&mut self, nodelay: bool) -> io::Result<()>;
    fn nodelay(&self) -> io::Result<bool>;
    fn set_ttl(&mut self, ttl: u32) -> io::Result<()>;
    fn ttl(&self) -> io::Result<u32>;
    fn set_nonblocking(&mut self, nonblocking: bool) -> io::Result<()>;

    fn try_clone(&self) -> io::Result<Self>
    where
        Self: Sized;
    fn take_error(&self) -> io::Result<Option<io::Error>>;
}

impl NetStream for TcpStream {}
impl NetConnection for TcpStream {
    type Addr = NetAddr<InetHost>;

    fn connect_blocking(addr: Self::Addr) -> io::Result<Self> {
        TcpStream::connect(addr)
    }

    #[cfg(feature = "connect_nonblocking")]
    fn connect_nonblocking(addr: Self::Addr) -> io::Result<Self> {
        Ok(socket2::Socket::connect_nonblocking(addr)?.into())
    }

    fn shutdown(&mut self, how: Shutdown) -> io::Result<()> {
        TcpStream::shutdown(self, how)
    }

    fn remote_addr(&self) -> Self::Addr {
        TcpStream::peer_addr(self).expect("TCP stream doesn't know remote peer address").into()
    }

    fn local_addr(&self) -> Self::Addr {
        TcpStream::local_addr(self).expect("TCP stream doesn't has local address").into()
    }

    fn set_read_timeout(&mut self, dur: Option<Duration>) -> io::Result<()> {
        TcpStream::set_read_timeout(self, dur)
    }
    fn set_write_timeout(&mut self, dur: Option<Duration>) -> io::Result<()> {
        TcpStream::set_write_timeout(self, dur)
    }
    fn read_timeout(&self) -> io::Result<Option<Duration>> {
        TcpStream::read_timeout(self)
    }
    fn write_timeout(&self) -> io::Result<Option<Duration>> {
        TcpStream::write_timeout(self)
    }

    fn peek(&self, buf: &mut [u8]) -> io::Result<usize> {
        TcpStream::peek(self, buf)
    }

    fn set_nodelay(&mut self, nodelay: bool) -> io::Result<()> {
        TcpStream::set_nodelay(self, nodelay)
    }
    fn nodelay(&self) -> io::Result<bool> {
        TcpStream::nodelay(self)
    }
    fn set_ttl(&mut self, ttl: u32) -> io::Result<()> {
        TcpStream::set_ttl(self, ttl)
    }
    fn ttl(&self) -> io::Result<u32> {
        TcpStream::ttl(self)
    }
    fn set_nonblocking(&mut self, nonblocking: bool) -> io::Result<()> {
        TcpStream::set_nonblocking(self, nonblocking)
    }

    fn try_clone(&self) -> io::Result<TcpStream> {
        TcpStream::try_clone(self)
    }
    fn take_error(&self) -> io::Result<Option<io::Error>> {
        TcpStream::take_error(self)
    }
}

#[cfg(feature = "socket2")]
impl NetStream for socket2::Socket {}
#[cfg(feature = "socket2")]
impl NetConnection for socket2::Socket {
    type Addr = NetAddr<InetHost>;

    fn connect_blocking(addr: Self::Addr) -> io::Result<Self> {
        TcpStream::connect(addr).map(socket2::Socket::from)
    }

    #[cfg(feature = "connect_nonblocking")]
    fn connect_nonblocking(addr: Self::Addr) -> io::Result<Self> {
        let addr = addr.to_socket_addrs()?.next().ok_or(io::ErrorKind::AddrNotAvailable)?;
        let socket =
            socket2::Socket::new(socket2::Domain::for_address(addr), socket2::Type::STREAM, None)?;
        socket.set_nonblocking(true)?;
        match socket2::Socket::connect(&socket, &addr.into()) {
            Ok(()) => {
                #[cfg(feature = "log")]
                log::debug!(target: "netservices", "Connected to {}", addr);
            }
            Err(e) if e.raw_os_error() == Some(libc::EINPROGRESS) => {
                #[cfg(feature = "log")]
                log::debug!(target: "netservices", "Connecting to {} in a non-blocking way", addr);
            }
            Err(e) if e.raw_os_error() == Some(libc::EALREADY) => {
                #[cfg(feature = "log")]
                log::error!(target: "netservices", "Can't connect to {}: address already in use", addr);
                return Err(io::Error::from(io::ErrorKind::AlreadyExists));
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                #[cfg(feature = "log")]
                log::error!(target: "netservices", "Can't connect to {} in a non-blocking way", addr);
            }
            Err(e) => {
                #[cfg(feature = "log")]
                log::debug!(target: "netservices", "Error connecting to {}: {}", addr, e);
                return Err(e);
            }
        }
        Ok(socket)
    }

    fn shutdown(&mut self, how: Shutdown) -> io::Result<()> {
        socket2::Socket::shutdown(self, how)
    }

    fn remote_addr(&self) -> Self::Addr {
        socket2::Socket::peer_addr(self)
            .expect("net stream must use only connections")
            .as_socket()
            .expect("net stream must use only connections")
            .into()
    }

    fn local_addr(&self) -> Self::Addr {
        socket2::Socket::local_addr(self)
            .expect("net stream doesn't has local socket")
            .as_socket()
            .expect("net stream doesn't has local socket")
            .into()
    }

    fn set_read_timeout(&mut self, dur: Option<Duration>) -> io::Result<()> {
        socket2::Socket::set_read_timeout(self, dur)
    }

    fn set_write_timeout(&mut self, dur: Option<Duration>) -> io::Result<()> {
        socket2::Socket::set_write_timeout(self, dur)
    }

    fn read_timeout(&self) -> io::Result<Option<Duration>> {
        socket2::Socket::read_timeout(self)
    }

    fn write_timeout(&self) -> io::Result<Option<Duration>> {
        socket2::Socket::write_timeout(self)
    }

    fn peek(&self, buf: &mut [u8]) -> io::Result<usize> {
        let mut buf2 = vec![MaybeUninit::<u8>::uninit(); buf.len()];
        let len = socket2::Socket::peek(self, &mut buf2)?;
        for i in 0..len {
            buf[i] = unsafe { buf2[i].assume_init() };
        }
        Ok(len)
    }

    fn set_nodelay(&mut self, nodelay: bool) -> io::Result<()> {
        socket2::Socket::set_nodelay(self, nodelay)
    }

    fn nodelay(&self) -> io::Result<bool> {
        socket2::Socket::nodelay(self)
    }

    fn set_ttl(&mut self, ttl: u32) -> io::Result<()> {
        socket2::Socket::set_ttl(self, ttl)
    }

    fn ttl(&self) -> io::Result<u32> {
        socket2::Socket::ttl(self)
    }

    fn set_nonblocking(&mut self, nonblocking: bool) -> io::Result<()> {
        socket2::Socket::set_nonblocking(self, nonblocking)
    }

    fn try_clone(&self) -> io::Result<Self> {
        socket2::Socket::try_clone(self)
    }

    fn take_error(&self) -> io::Result<Option<io::Error>> {
        socket2::Socket::take_error(self)
    }
}
