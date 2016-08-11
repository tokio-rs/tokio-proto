use io::{Framed, Parse, Serialize, Readiness};
use bytes::{BlockBuf};
use std::io;

/// Convenience trait representing a bi-directional Tokio aware IO stream.
pub trait Stream: io::Read + io::Write + Readiness {
    /// Frame this stream
    fn frame<P, S>(self, parse: P, serialize: S) -> Framed<Self, P, S>
        where Self: Sized,
              P: Parse,
              S: Serialize,
    {
        Framed::new(self, parse, serialize, BlockBuf::default(), BlockBuf::default())
    }
}

impl<T: io::Read + io::Write + Readiness> Stream for T {
}
