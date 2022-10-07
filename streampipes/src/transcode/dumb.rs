//! Dumb transcoder which does nothing to the data. Useful for testing purposes.
//! It also does some dumb three-staged handshake.

use std::io;

use super::{Decode, Encode, TranscodedStream};

pub type DumbTranscodedStream<R, W> = TranscodedStream<R, W, DumbDecoder, DumbEncoder>;

pub struct DumbEncoder;
pub struct DumbDecoder;

impl Encode for DumbEncoder {
    fn encrypt_byte(&mut self, byte: u8) -> u8 {
        byte
    }
}

impl Decode for DumbDecoder {
    fn decrypt(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        Ok(buf.len())
    }
}
