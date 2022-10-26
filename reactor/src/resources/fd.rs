use crate::{InputEvent, Resource};

/// Resource subtype which is able to handle events from the file descriptors
pub trait FdResource: Resource + Sized {
    /// Blocks on reading input event from the file descriptor resource
    fn read_input_event(&mut self) -> Result<InputEvent<Self>, Self::Error>;

    /// Blocks on reading output event from the file descriptor resource
    fn read_output_event(&mut self) -> Result<InputEvent<Self>, Self::Error>;
}
