use crate::NetSession;

#[derive(Debug, Display)]
#[display("{error}")]
pub struct SplitIoError<T: SplitIo> {
    pub original: T,
    pub error: std::io::Error,
}

impl<T: SplitIo + std::fmt::Debug> std::error::Error for SplitIoError<T> {}

pub trait SplitIo: Sized {
    type Read: std::io::Read + Sized;
    type Write: std::io::Write + Sized;

    /// # Panics
    ///
    /// If the split operation is not possible
    fn split_io(self) -> Result<(Self::Read, Self::Write), SplitIoError<Self>>;
    fn from_split_io(read: Self::Read, write: Self::Write) -> Self;
}

pub struct NetReader<S: NetSession> {
    session: <S as SplitIo>::Read,
}

impl<S: NetSession> io::Read for NetReader<S> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.session.read(buf)
    }
}

pub struct NetWriter<S: NetSession> {
    session: <S as SplitIo>::Write,
}

impl<S: NetSession> io::Write for NetWriter<S> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.session.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.session.flush()
    }
}

impl SplitIo for TcpStream {
    type Read = Self;
    type Write = Self;

    fn split_io(self) -> Result<(Self::Read, Self::Write), SplitIoError<Self>> {
        match self.try_clone() {
            Ok(clone) => Ok((clone, self)),
            Err(error) => Err(SplitIoError {
                original: self,
                error,
            }),
        }
    }

    fn from_split_io(_read: Self::Read, write: Self::Write) -> Self {
        write
    }
}

#[cfg(feature = "connect_nonblocking")]
impl SplitIo for socket2::Socket {
    type Read = Self;
    type Write = Self;

    fn split_io(self) -> Result<(Self::Read, Self::Write), SplitIoError<Self>> {
        match self.try_clone() {
            Ok(clone) => Ok((clone, self)),
            Err(error) => Err(SplitIoError {
                original: self,
                error,
            }),
        }
    }

    fn from_split_io(read: Self::Read, write: Self::Write) -> Self {
        // TODO: Do a better detection of unrelated join
        if read.as_raw_fd() != write.as_raw_fd() {
            panic!("attempt to join unrelated streams")
        }
        read
    }
}
