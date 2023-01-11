use std::io;
use std::io::{Read, Write};
use std::net::TcpStream;
use std::os::fd::{AsRawFd, RawFd};

use cyphernet::noise::framing::{NoiseDecryptor, NoiseEncryptor, NoiseState, NoiseTranscoder};

use crate::{NetSession, NetStream, SplitIo, SplitIoError};

#[derive(Debug)]
pub struct NoiseSession<N: NoiseState, S: NetSession = TcpStream> {
    inner: S,
    transcoder: NoiseTranscoder<N>,
}

impl<N: NoiseState, S: NetSession> AsRawFd for NoiseSession<N, S> {
    fn as_raw_fd(&self) -> RawFd {
        self.inner.as_raw_fd()
    }
}

impl<N: NoiseState, S: NetSession> NetStream for NoiseSession<N, S> {}
impl<N: NoiseState, S: NetSession> NetSession for NoiseSession<N, S> {
    type Inner = S;
    type Connection = S::Connection;
    type Artifact = N::StaticKey;

    fn artifact(&self) -> Option<Self::Artifact> {
        todo!()
    }

    fn as_connection(&self) -> &Self::Connection {
        self.inner.as_connection()
    }

    fn disconnect(self) -> io::Result<()> {
        self.inner.disconnect()
    }
}

impl<N: NoiseState, S: NetSession> Read for NoiseSession<N, S> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if !self.transcoder.is_handshake_complete() {
            let mut input = vec![0u8; self.transcoder.next_handshake_len()];
            self.inner.read_exact(&mut input)?;

            #[cfg(feature = "log")]
            log::trace!(target: "noise-session", "Received {input:02x?}");

            let act = self
                .transcoder
                .advance_handshake(&input)
                .map_err(|err| io::Error::new(io::ErrorKind::ConnectionAborted, err))?;

            return match act {
                None => Ok(0),
                Some(act) => {
                    #[cfg(feature = "log")]
                    log::trace!(target: "noise-session", "Sent {act:02x?}");

                    self.inner.write_all(&act)?;

                    Ok(0)
                }
            };
        }
        self.inner.read(buf)
    }
}

impl<N: NoiseState, S: NetSession> Write for NoiseSession<N, S> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if !self.transcoder.is_handshake_complete() {
            let act = self
                .transcoder
                .advance_handshake(&[])
                .map_err(|err| io::Error::new(io::ErrorKind::ConnectionAborted, err))?;

            if let Some(next_act) = act {
                #[cfg(feature = "log")]
                log::trace!(target: "noise-session", "Sent {next_act:02x?}");

                self.inner.write_all(&next_act)?
            }

            return Err(io::ErrorKind::Interrupted.into());
        }
        self.inner.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

#[derive(Debug)]
pub struct NoiseReader<N: NoiseState, S: NetSession = TcpStream> {
    reader: S::Read,
    decryptor: NoiseDecryptor<N>,
}

#[derive(Debug)]
pub struct NoiseWriter<N: NoiseState, S: NetSession = TcpStream> {
    writer: S::Write,
    encryptor: NoiseEncryptor<N>,
}

impl<N: NoiseState, S: NetSession> Read for NoiseReader<N, S> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.reader.read(buf)
    }
}

impl<N: NoiseState, S: NetSession> Write for NoiseWriter<N, S> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.writer.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.writer.flush()
    }
}

impl<N: NoiseState, S: NetSession> SplitIo for NoiseSession<N, S>
where
    S: SplitIo,
{
    type Read = NoiseReader<N, S>;
    type Write = NoiseWriter<N, S>;

    fn split_io(mut self) -> Result<(Self::Read, Self::Write), SplitIoError<Self>> {
        if !self.transcoder.is_handshake_complete() {
            return Err(SplitIoError {
                original: self,
                error: io::ErrorKind::NotConnected.into(),
            });
        }

        let (reader, writer) = match self.inner.split_io() {
            Ok((reader, writer)) => (reader, writer),
            Err(SplitIoError { original, error }) => {
                self.inner = original;
                return Err(SplitIoError {
                    original: self,
                    error,
                });
            }
        };

        let (encryptor, decryptor) = match self.transcoder.try_into_split() {
            Ok((encryptor, decryptor)) => (encryptor, decryptor),
            Err((transcoder, _)) => {
                self.transcoder = transcoder;
                self.inner = S::from_split_io(reader, writer);
                return Err(SplitIoError {
                    original: self,
                    error: io::ErrorKind::NotConnected.into(),
                });
            }
        };

        Ok((
            NoiseReader { reader, decryptor },
            NoiseWriter { writer, encryptor },
        ))
    }

    fn from_split_io(read: Self::Read, write: Self::Write) -> Self {
        Self {
            transcoder: NoiseTranscoder::with_split(write.encryptor, read.decryptor),
            inner: S::from_split_io(read.reader, write.writer),
        }
    }
}
