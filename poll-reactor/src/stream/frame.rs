use std::collections::VecDeque;
use std::io::{Error, Read, Write};

use crate::stream::Stream;

pub trait Frame: Stream {
    type Message;

    fn push(&mut self, msg: Self::Message);
    fn pop(&mut self) -> Result<Option<Self::Message>, Error>;
    fn queue_len(&self) -> usize;
}

#[derive(Clone, Debug, Default)]
pub struct VecFrame {
    read_queue: VecDeque<u8>,
    write_queue: VecDeque<u8>,
}

impl VecFrame {
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
}

impl Frame for VecFrame {
    type Message = Vec<u8>;

    fn push(&mut self, msg: Self::Message) {
        self.write_queue.extend(msg);
    }

    fn pop(&mut self) -> Result<Option<Self::Message>, Error> {
        Ok(Some(self.read_queue.drain(..).collect()))
    }

    fn queue_len(&self) -> usize {
        self.write_queue.len()
    }
}

impl Read for VecFrame {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.write_queue.read(buf)
    }
}

impl Write for VecFrame {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.read_queue.write(buf)
    }

    fn flush(&mut self) -> std::io::Result<()> {
        // Do nothing
        Ok(())
    }
}
