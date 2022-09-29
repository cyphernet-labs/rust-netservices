//! Multiplexed framed packets

use super::FramedStream;
use crate::frame::Frame;

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Multiplexed<Payload: Frame> {
    pub channel: u16,
    pub payload: Payload,
}

#[derive(Debug)]
pub enum MultiplexedError<Payload: Frame> {
    NoChannel,
    PayloadParse(Payload::Error),
}

impl<Payload: Frame> Frame for Multiplexed<Payload> {
    type Error = MultiplexedError<Payload>;

    fn parse(payload: &[u8]) -> Result<Self, Self::Error> {
        if payload.len() < 2 {
            return Err(MultiplexedError::NoChannel);
        }
        let mut channel = [0u8; 2];
        channel.copy_from_slice(&payload[..2]);
        let channel = u16::from_le_bytes(channel);
        let payload = Payload::parse(payload).map_err(MultiplexedError::PayloadParse)?;
        Ok(Self { channel, payload })
    }
}

pub type Multiplexed64KbFrames<R, W, D, E, Payload> =
    FramedStream<R, W, D, E, Multiplexed<Payload>, 2u8>;
