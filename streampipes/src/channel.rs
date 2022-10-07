use std::collections::VecDeque;
use std::io;

pub struct ChannelStream {
    buffer: VecDeque<u8>,
}

impl io::Read for ChannelStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.len() > self.buffer.len() {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        let len = buf.len();
        for byte in buf {
            *byte = self.buffer.pop_front().expect("length check failed");
        }
        Ok(len)
    }
}

impl io::Write for ChannelStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.buffer.extend(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        // Nothing to do
        Ok(())
    }
}
