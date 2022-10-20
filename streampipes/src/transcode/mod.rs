//! Transcoders perform cypering/decyphering of the read or write stream, when
//! each byte is replaced with a different exactly one byte. Transcoders can
//! have a handshake phase.

pub mod dumb;

use std::io::{Read, Write};
use std::{io, slice};

// We pass to it a VecDeque instance as a reader
pub struct TranscodedStream<R, W, D, E>
where
    R: Read + Send,
    W: Write + Send,
    D: Decode,
    E: Encode,
{
    reader: R,
    writer: W,
    decoder: D,
    encoder: E,
}

impl<R, W, D, E> TranscodedStream<R, W, D, E>
where
    R: Read + Send,
    W: Write + Send,
    D: Decode,
    E: Encode,
{
    pub fn with(reader: R, writer: W, decoder: D, encoder: E) -> Self {
        Self {
            reader,
            writer,
            decoder,
            encoder,
        }
    }
}

impl<R, W, D, E> TranscodedStream<R, W, D, E>
where
    R: Read + Write + Send,
    W: Write + Send,
    D: Decode,
    E: Encode,
{
    pub fn push_bytes(&mut self, bytes: impl AsRef<[u8]>) -> io::Result<usize> {
        self.reader.write(bytes.as_ref())
    }
}

/// In-place slice encoder
pub trait Encode: Send + Sized {
    fn encrypt_iter<'me, 'slice>(
        &'me mut self,
        buf: &'slice [u8],
    ) -> EncodeIter<'slice, 'me, Self> {
        EncodeIter {
            slice: buf.into_iter(),
            encoder: self,
        }
    }
    fn encrypt_byte(&mut self, byte: u8) -> u8;
}

pub struct EncodeIter<'slice, 'encryptor, E>
where
    E: 'encryptor + Encode,
{
    pub(self) slice: slice::Iter<'slice, u8>,
    pub(self) encoder: &'encryptor mut E,
}

impl<'slice, 'encryptor, E> Iterator for EncodeIter<'slice, 'encryptor, E>
where
    E: 'encryptor + Encode,
{
    type Item = u8;

    fn next(&mut self) -> Option<Self::Item> {
        self.slice
            .next()
            .map(|byte| self.encoder.encrypt_byte(*byte))
    }
}

/// In-place slice decoder
pub trait Decode: Send {
    fn decrypt(&mut self, buf: &mut [u8]) -> io::Result<usize>;
}

impl<R, W, D, E> Read for TranscodedStream<R, W, D, E>
where
    R: Read + Send,
    W: Write + Send,
    D: Decode,
    E: Encode,
{
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.reader.read_exact(buf)?;
        let size1 = buf.len();
        let size2 = self.decoder.decrypt(buf)?;
        debug_assert_eq!(size1, size2);
        Ok(size1)
    }
}

impl<R, W, D, E> Write for TranscodedStream<R, W, D, E>
where
    R: Read + Send,
    W: Write + Send,
    D: Decode,
    E: Encode,
{
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut len = 0usize;
        for byte in self.encoder.encrypt_iter(buf) {
            self.writer.write_all(&[byte])?;
            len += 1;
        }
        Ok(len)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.writer.flush()
    }
}
