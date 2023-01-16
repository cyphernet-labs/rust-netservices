use std::io;
use std::net::TcpStream;

use crate::{NetConnection, NetSession, NetStateMachine};

#[derive(Debug, Display)]
#[display("{error}")]
pub struct SplitIoError<T: SplitIo> {
    pub original: T,
    pub error: io::Error,
}

impl<T: SplitIo + std::fmt::Debug> std::error::Error for SplitIoError<T> {}

pub trait SplitIo: Sized {
    type Read: io::Read + Sized;
    type Write: io::Write + Sized;

    /// # Panics
    ///
    /// If the split operation is not possible
    fn split_io(self) -> Result<(Self::Read, Self::Write), SplitIoError<Self>>;
    fn from_split_io(read: Self::Read, write: Self::Write) -> Self;
}

pub struct NetReader<S: NetSession> {
    pub(crate) unique_id: u64,
    pub(crate) reader: <S as SplitIo>::Read,
}

impl<S: NetSession> io::Read for NetReader<S> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.reader.read(buf)
    }
}

pub struct NetWriter<M: NetStateMachine, S: NetSession> {
    pub(crate) unique_id: u64,
    pub(crate) state: M,
    pub(crate) writer: <S as SplitIo>::Write,
}

impl<M: NetStateMachine, S: NetSession> io::Write for NetWriter<M, S> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.writer.write(buf)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.writer.flush()
    }
}

pub struct TcpReader<C: NetConnection> {
    unique_id: u64,
    connection: C,
}

impl<C: NetConnection> io::Read for TcpReader<C> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.connection.read(buf)
    }
}

pub struct TcpWriter<C: NetConnection> {
    unique_id: u64,
    connection: C,
}

impl<C: NetConnection> io::Write for TcpWriter<C> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.connection.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.connection.flush()
    }
}

impl SplitIo for TcpStream {
    type Read = TcpReader<Self>;
    type Write = TcpWriter<Self>;

    fn split_io(self) -> Result<(Self::Read, Self::Write), SplitIoError<Self>> {
        match self.try_clone() {
            Ok(clone) => {
                let unique_id = rand::random();
                let reader = TcpReader {
                    unique_id,
                    connection: clone,
                };
                let writer = TcpWriter {
                    unique_id,
                    connection: self,
                };
                Ok((reader, writer))
            }
            Err(error) => Err(SplitIoError {
                original: self,
                error,
            }),
        }
    }

    fn from_split_io(read: Self::Read, write: Self::Write) -> Self {
        if read.unique_id != write.unique_id {
            panic!("joining TcpStreams which were not produced by the same split_io()")
        }
        write.connection
    }
}

#[cfg(feature = "connect_nonblocking")]
impl SplitIo for socket2::Socket {
    type Read = TcpReader<Self>;
    type Write = TcpWriter<Self>;

    fn split_io(self) -> Result<(Self::Read, Self::Write), SplitIoError<Self>> {
        match self.try_clone() {
            Ok(clone) => {
                let unique_id = rand::random();
                let reader = TcpReader {
                    unique_id,
                    connection: clone,
                };
                let writer = TcpWriter {
                    unique_id,
                    connection: self,
                };
                Ok((reader, writer))
            }
            Err(error) => Err(SplitIoError {
                original: self,
                error,
            }),
        }
    }

    fn from_split_io(read: Self::Read, write: Self::Write) -> Self {
        if read.unique_id != write.unique_id {
            panic!("joining TcpStreams which were not produced by the same split_io()")
        }
        write.connection
    }
}
