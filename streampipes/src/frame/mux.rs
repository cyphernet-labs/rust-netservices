//! Multiplexed framed packets

use super::FramedStream;
use crate::frame::Frame;
use std::convert::Infallible;

impl Frame for Vec<u8> {
    type Error = Infallible;

    fn parse(payload: &[u8]) -> Result<Self, Self::Error> {
        Ok(payload.to_vec())
    }
}

impl Frame for Box<[u8]> {
    type Error = Infallible;

    fn parse(payload: &[u8]) -> Result<Self, Self::Error> {
        Ok(Box::from(payload))
    }
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub struct Multiplexed<Payload: Frame> {
    pub channel: u16,
    pub payload: Payload,
}

impl<Payload> Iterator for Multiplexed<Payload>
where
    Payload: Frame + Iterator<Item = u8>,
{
    type Item = Payload::Item;

    fn next(&mut self) -> Option<Self::Item> {
        self.payload.next()
    }
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

pub type Multiplexed64KbData<R, W, D, E> = Multiplexed64KbFrames<R, W, D, E, Box<[u8]>>;
