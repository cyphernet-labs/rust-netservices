//! Multiplexed framed packets

use super::FramedStream;
use crate::frame::Frame;

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct MultiplexedFrame {
    pub channel: u16,
    pub payload: Box<[u8]>,
}

#[derive(Debug)]
pub struct InvalidMultiplexedFrame;

impl Frame for MultiplexedFrame {
    type Error = InvalidMultiplexedFrame;

    fn parse(payload: &[u8]) -> Result<Self, Self::Error> {
        if payload.len() < 2 {
            return Err(InvalidMultiplexedFrame);
        }
        let mut channel = [0u8; 2];
        channel.copy_from_slice(&payload[..2]);
        let channel = u16::from_le_bytes(channel);
        Ok(Self {
            channel,
            payload: Box::from(&payload[2..]),
        })
    }
}

pub type Multiplexed64KbFrames<R, W, D, E> = FramedStream<R, W, D, E, MultiplexedFrame, 2u8>;
