use std::collections::VecDeque;
use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::os::unix::io::{AsRawFd, RawFd};
use std::time;

use socket2::{Domain, Socket, Type};

use crate::actors::stdtcp::TcpAction;
use crate::actors::IoEv;
use crate::{Actor, Controller, Layout};

// TODO: Move to context
/// Maximum time to wait when reading from a socket.
const READ_TIMEOUT: time::Duration = time::Duration::from_secs(6);
// TODO: Move to context
/// Maximum time to wait when writing to a socket.
const WRITE_TIMEOUT: time::Duration = time::Duration::from_secs(3);

pub struct SocketConnection<L: Layout> {
    socket: Socket,
    queue: VecDeque<u8>,
    read_buf: Vec<u8>,
    pub(super) controller: Controller<L>,
    pub(super) is_inbound: bool,
}

impl<L: Layout> SocketConnection<L> {
    pub fn connect(addr: SocketAddr, controller: Controller<L>) -> io::Result<Self> {
        let read_buf = Vec::with_capacity(u16::MAX as usize);

        let domain = if addr.is_ipv4() {
            Domain::IPV4
        } else {
            Domain::IPV6
        };
        let socket = Socket::new(domain, Type::STREAM, None)?;

        socket.set_read_timeout(Some(READ_TIMEOUT))?;
        socket.set_write_timeout(Some(WRITE_TIMEOUT))?;
        socket.set_nonblocking(true)?;

        match socket.connect(&addr.into()) {
            Ok(()) => {}
            Err(e) if e.raw_os_error() == Some(libc::EINPROGRESS) => {}
            Err(e) if e.raw_os_error() == Some(libc::EALREADY) => {
                return Err(io::Error::from(io::ErrorKind::AlreadyExists))
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {}
            Err(e) => return Err(e),
        }

        Ok(Self {
            socket,
            queue: empty!(),
            read_buf,
            controller,
            is_inbound: false,
        })
    }

    pub fn accept(stream: TcpStream, controller: Controller<L>) -> io::Result<Self> {
        let read_buf = Vec::with_capacity(u16::MAX as usize);

        let socket = Socket::from(stream);

        socket.set_read_timeout(Some(READ_TIMEOUT))?;
        socket.set_write_timeout(Some(WRITE_TIMEOUT))?;
        socket.set_nonblocking(true)?;

        Ok(Self {
            socket,
            queue: empty!(),
            read_buf,
            controller,
            is_inbound: true,
        })
    }
}

impl<L: Layout> Actor for SocketConnection<L> {
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
        self.socket.as_raw_fd()
    }

    fn io_ready(&mut self, io: IoEv) -> Result<(), Self::Error> {
        if io.is_readable {
            self.flush()?;
        }
        if io.is_writable {
            let len = self.socket.read(&mut self.read_buf)?;
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

impl<L: Layout> Read for SocketConnection<L> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.queue.read(buf)
    }
}

impl<L: Layout> Write for SocketConnection<L> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.socket.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.socket.flush()
    }
}

impl<L: Layout> AsRawFd for SocketConnection<L> {
    fn as_raw_fd(&self) -> RawFd {
        self.socket.as_raw_fd()
    }
}
