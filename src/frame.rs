use std::collections::VecDeque;
use std::io::{self, Read, Write};

use crate::IoStream;

pub trait Marshall: IoStream + Default {
    type Message;
    type Error;

    fn push(&mut self, msg: Self::Message);
    fn pop(&mut self) -> Result<Option<Self::Message>, Self::Error>;
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

impl Marshall for VecFrame {
    type Message = Vec<u8>;
    type Error = ();

    fn push(&mut self, msg: Self::Message) {
        self.write_queue.extend(msg);
    }

    fn pop(&mut self) -> Result<Option<Self::Message>, Self::Error> {
        Ok(Some(self.read_queue.drain(..).collect()))
    }

    fn queue_len(&self) -> usize {
        self.write_queue.len()
    }
}

impl Read for VecFrame {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.write_queue.read(buf)
    }
}

impl Write for VecFrame {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.read_queue.write(buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        // Do nothing
        Ok(())
    }
}
