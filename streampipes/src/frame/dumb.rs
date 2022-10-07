use super::FramedStream;
use crate::frame::Frame;
use crate::transcode::dumb::{DumbDecoder, DumbEncoder};
use std::convert::Infallible;

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Debug)]
pub enum DumbFrame64Kb {
    Dumb(Vec<u8>),
}

impl AsRef<[u8]> for DumbFrame64Kb {
    fn as_ref(&self) -> &[u8] {
        match self {
            DumbFrame64Kb::Dumb(data) => data.as_ref(),
        }
    }
}

impl Frame for DumbFrame64Kb {
    type Error = Infallible;

    fn parse(payload: &[u8]) -> Result<Self, Self::Error> {
        Ok(Self::Dumb(payload.to_vec()))
    }
}

pub type DumbFramed64KbStream<R, W> =
    FramedStream<R, W, DumbDecoder, DumbEncoder, DumbFrame64Kb, 2u8>;

#[cfg(test)]
mod test {
    use crate::frame::dumb::DumbFramed64KbStream;
    use crate::transcode::dumb::{DumbDecoder, DumbEncoder};
    use std::io;
    use std::io::Write;

    #[test]
    fn test() {
        let reader = io::Cursor::new(Vec::<u8>::new());
        let writer = Vec::<u8>::new();
        let mut stream = DumbFramed64KbStream::with(reader, writer, DumbDecoder, DumbEncoder);

        let data = [
            0x02, 0x00, b'a', b'b', 0x05, 0x00, b'M', b'a', b'x', b'i', b'm',
        ];
        let mut expected_payloads = [&b"ab"[..], &b"Maxim"[..]].into_iter();

        for byte in data {
            // Writing data byte by byte, ensuring that the reading is not broken
            stream.write(&[byte]).unwrap();
            for frame in &mut stream {
                assert_eq!(frame.as_ref(), expected_payloads.next().unwrap().as_ref());
            }
        }
    }
}
