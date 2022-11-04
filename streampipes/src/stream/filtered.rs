use amplify::Bipolar;
use std::io::{self, Read, Write};

use crate::stream::Stream;

pub trait ReadFilter: Read {}

pub trait WriteFilter {}

pub trait Filter: ReadFilter + WriteFilter {}

impl<T> Filter for T where T: ReadFilter + WriteFilter {}

pub struct FilteredReader<R: Read, F: ReadFilter> {
    reader: R,
    filter: F,
}

pub struct FilteredWriter<W: Write, F: WriteFilter> {
    writer: W,
    filter: F,
}

pub struct FilteredStream<S: Stream, F: Filter> {
    stream: S,
    filter: F,
}

impl<S: Stream, F: Filter> FilteredStream<S, F> {}

impl<S, F> Bipolar for FilteredStream<S, F>
where
    S: Stream + Bipolar,
    F: Filter + Bipolar,
{
    type Left = FilteredReader<S::Left, F::Left>;
    type Right = FilteredWriter<S::Right, F::Right>;

    fn join(left: Self::Left, right: Self::Right) -> Self {
        Self {
            stream: S::join(left.reader, right.writer),
            filter: F::join(left.filter, right.filter),
        }
    }

    fn split(self) -> (Self::Left, Self::Right) {
        let (reader, writer) = self.stream.split();
        let (fread, fwrite) = self.filter.split();
        (
            FilteredReader {
                reader,
                filter: fread,
            },
            FilteredWriter {
                writer,
                filter: fwrite,
            },
        )
    }
}
