use std::io::{Read, Write};
use std::marker::PhantomData;
use std::{io, slice};

// We pass to it a VecDeque instance as a reader
pub struct EncryptedStream<R, W, D, E>
where
    R: Read + Send,
    W: Write + Send,
    D: Decrypt,
    E: Encrypt,
{
    reader: R,
    writer: W,
    decryptor: D,
    encryptor: E,
}

/// In-place slice encryptor
pub trait Encrypt: Send + Sized {
    fn encrypt_iter<'me, 'slice>(
        &'me mut self,
        buf: &'slice [u8],
    ) -> EncryptIter<'slice, 'me, Self> {
        EncryptIter {
            slice: buf.into_iter(),
            encryptor: self,
        }
    }
    fn encrypt_byte(&mut self, byte: u8) -> u8;
}

pub struct EncryptIter<'slice, 'encryptor, E>
where
    E: 'encryptor + Encrypt,
{
    pub(self) slice: slice::Iter<'slice, u8>,
    pub(self) encryptor: &'encryptor mut E,
}

impl<'slice, 'encryptor, E> Iterator for EncryptIter<'slice, 'encryptor, E>
where
    E: 'encryptor + Encrypt,
{
    type Item = u8;

    fn next(&mut self) -> Option<Self::Item> {
        self.slice
            .next()
            .map(|byte| self.encryptor.encrypt_byte(*byte))
    }
}

/// In-place slice decryptor
pub trait Decrypt: Send {
    fn decrypt(&mut self, buf: &mut [u8]) -> io::Result<usize>;
}

impl<R, W, D, E> Read for EncryptedStream<R, W, D, E>
where
    R: Read + Send,
    W: Write + Send,
    D: Decrypt,
    E: Encrypt,
{
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let size1 = self.reader.read(buf)?;
        let size2 = self.decryptor.decrypt(buf)?;
        debug_assert_eq!(size1, size2);
        Ok(size1)
    }
}

impl<R, W, D, E> Write for EncryptedStream<R, W, D, E>
where
    R: Read + Send,
    W: Write + Send,
    D: Decrypt,
    E: Encrypt,
{
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut len = 0usize;
        for byte in self.encryptor.encrypt_iter(buf) {
            self.writer.write_all(&[byte])?;
            len += 1;
        }
        Ok(len)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.writer.flush()
    }
}

pub trait Frame: Sized {
    type Error;
    fn parse(payload: &[u8]) -> Result<Self, Self::Error>;
}

/// Processes individual message frames out of the stream.
///
/// The stream must follow the format of
/// <FRAME_PREFIX_BYTES-bit message length in little endian><message payload>.
pub struct FramedStream<R, W, D, E, F, const FRAME_PREFIX_BYTES: u8>
where
    R: Read + Send,
    W: Write + Send,
    D: Decrypt,
    E: Encrypt,
    F: Frame,
{
    stream: EncryptedStream<R, W, D, E>,
    _phantom: PhantomData<F>,
}

impl<R, W, D, E, F, const FRAME_PREFIX_BYTES: u8> FramedStream<R, W, D, E, F, FRAME_PREFIX_BYTES>
where
    R: Read + Send,
    W: Write + Send,
    D: Decrypt,
    E: Encrypt,
    F: Frame,
{
    const BYTE_LEN: usize = FRAME_PREFIX_BYTES as usize;
}

impl<R, W, D, E, F, const FRAME_PREFIX_BYTES: u8> Iterator
    for FramedStream<R, W, D, E, F, FRAME_PREFIX_BYTES>
where
    R: Read + Send,
    W: Write + Send,
    D: Decrypt,
    E: Encrypt,
    F: Frame,
{
    type Item = F;

    // TODO: Handle errors somehow
    fn next(&mut self) -> Option<Self::Item> {
        debug_assert!(Self::BYTE_LEN <= (usize::BITS / 8) as usize);
        let mut len = [0u8; (usize::BITS / 8u32) as usize];
        self.stream.read_exact(&mut len[..Self::BYTE_LEN]).ok()?;
        let len = usize::from_le_bytes(len);
        let mut buf = vec![0u8; len as usize];
        self.stream.read_exact(&mut buf).ok()?;
        F::parse(&buf).ok()
    }
}
