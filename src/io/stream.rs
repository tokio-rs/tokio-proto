use io::{Readiness};
use std::io;

/// Convenience trait representing a bi-directional Tokio aware IO stream.
pub trait Stream: io::Read + io::Write + Readiness {
}

impl<T: io::Read + io::Write + Readiness> Stream for T {
}
