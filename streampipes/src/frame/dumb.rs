use std::convert::Infallible;

use super::FramedStream;
use crate::frame::Frame;

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

pub type DumbFramed64KbStream<S> = FramedStream<S, DumbFrame64Kb, 2u8>;

#[cfg(test)]
mod test {
    use std::collections::VecDeque;
    use std::io::Write;

    use crate::frame::dumb::DumbFramed64KbStream;

    #[test]
    fn test() {
        let mut stream = DumbFramed64KbStream::with(VecDeque::new());

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
