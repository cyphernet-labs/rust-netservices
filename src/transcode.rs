use std::io::{Read, Write};
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
