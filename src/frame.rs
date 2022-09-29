use std::io::{Read, Write};
use std::marker::PhantomData;

use crate::transcode::{Decrypt, Encrypt, EncryptedStream};

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
