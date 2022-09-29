use super::FramedStream;
use crate::frame::Frame;
use crate::transcode::dumb::{DumbDecoder, DumbEncoder};
use std::convert::Infallible;

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum DumbFrame64Kb {
    Dumb(Vec<u8>),
}

impl Frame for DumbFrame64Kb {
    type Error = Infallible;

    fn parse(payload: &[u8]) -> Result<Self, Self::Error> {
        Ok(Self::Dumb(payload.to_vec()))
    }
}

pub type DumbFramed64KbStream<R, W> =
    FramedStream<R, W, DumbDecoder, DumbEncoder, DumbFrame64Kb, 2u8>;
