pub mod dumb;
pub mod mux;

use std::io::{self, Read, Write};
use std::marker::PhantomData;

use crate::Stream;

pub trait Frame: Sized {
    type Error;
    fn parse(payload: &[u8]) -> Result<Self, Self::Error>;
}

/// Processes individual message frames out of the stream.
///
/// The stream must follow the format of
/// <FRAME_PREFIX_BYTES-bit message length in little endian><message payload>.
pub struct FramedStream<S: Stream, F: Frame, const FRAME_PREFIX_BYTES: u8> {
    stream: S,
    _phantom: PhantomData<F>,
}

impl<S: Stream, F: Frame, const FRAME_PREFIX_BYTES: u8> FramedStream<S, F, FRAME_PREFIX_BYTES> {
    pub fn with(stream: S) -> Self {
        Self {
            stream,
            _phantom: Default::default(),
        }
    }
}

impl<S: Stream, F: Frame, const FRAME_PREFIX_BYTES: u8> Read
    for FramedStream<S, F, FRAME_PREFIX_BYTES>
{
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.stream.read(buf)
    }
}

impl<S: Stream, F: Frame, const FRAME_PREFIX_BYTES: u8> Write
    for FramedStream<S, F, FRAME_PREFIX_BYTES>
{
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.stream.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.stream.flush()
    }
}

impl<'me, S: Stream, F: Frame, const FRAME_PREFIX_BYTES: u8> Iterator
    for &'me mut FramedStream<S, F, FRAME_PREFIX_BYTES>
{
    type Item = F;

    // TODO: Ensure that if the buffer does not contain a whole frame
    //       the reader position has not advanced.
    fn next(&mut self) -> Option<Self::Item> {
        debug_assert!(FRAME_PREFIX_BYTES <= (usize::BITS / 8) as u8);
        let mut len = [0u8; (usize::BITS / 8u32) as usize];
        self.stream
            .read_exact(&mut len[..(FRAME_PREFIX_BYTES as usize)])
            .ok()?;
        let len = usize::from_le_bytes(len);
        let mut buf = vec![0u8; len as usize];
        self.stream.read_exact(&mut buf).ok()?;
        F::parse(&buf).ok()
    }
}
