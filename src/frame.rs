use std::collections::VecDeque;
use std::io::{self, Read, Write};

pub trait Frame: Sized {
    type Error: std::error::Error;

    /// Reads frame from the stream.
    ///
    /// If the stream doesn't contain the whole message yet must return `Ok(None)`
    fn unmarshall(reader: impl Read) -> Result<Option<Self>, Self::Error>;
    fn marshall(&self, writer: impl Write) -> Result<usize, Self::Error>;
}

#[derive(Clone, Debug, Default)]
pub struct Marshaller {
    read_queue: VecDeque<u8>,
    write_queue: VecDeque<u8>,
}

impl Marshaller {
    pub fn new() -> Self {
        Self {
            read_queue: VecDeque::new(),
            write_queue: VecDeque::new(),
        }
    }

    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            read_queue: VecDeque::with_capacity(capacity),
            write_queue: VecDeque::with_capacity(capacity),
        }
    }

    pub fn push<F: Frame>(&mut self, frame: F) {
        frame
            .marshall(&mut self.write_queue)
            .expect("in-memory write operation");
    }

    pub fn pop<F: Frame>(&mut self) -> Result<Option<F>, F::Error> {
        let slice = self.read_queue.make_contiguous();
        let mut cursor = io::Cursor::new(slice.as_ref());
        let frame = F::unmarshall(&mut cursor)?;
        let pos = cursor.position() as usize;
        if frame.is_some() {
            self.read_queue.drain(..pos);
        }
        return Ok(frame);
    }

    pub fn queue_len(&self) -> usize {
        self.write_queue.len()
    }
}

impl Read for Marshaller {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.write_queue.read(buf)
    }
}

impl Write for Marshaller {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.read_queue.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        // Do nothing
        Ok(())
    }
}
