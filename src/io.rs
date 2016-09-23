use bytes::{Buf, MutBuf};
use bytes::buf::{ReadExt, WriteExt};
use futures::{Poll, Async};
use std::io;

/// A refinement of `std::io::Read` for reading from non-blocking sources.
///
/// When reading from a non-blocking source, receiving a
/// `ErrorKind::WouldBlock` is expected and not an error. A such, it is useful
/// to split out the would-block case from the other error cases.
pub trait TryRead: io::Read {

    /// Pull some bytes from this source into the specified buffer, returning
    /// how many bytes were read.
    ///
    /// If the source is not able to perform the operation due to not having
    /// any bytes to read, `Ok(Async::NotReady)` is returned.
    ///
    /// Aside from the signature, behavior is identical to `std::io::Read`. For
    /// more details, read the `std::io::Read` documentation.
    fn try_read(&mut self, buf: &mut [u8]) -> Poll<usize, io::Error>;

    /// Pull some bytes from this source into the specified `Buf`, returning
    /// how many bytes were read.
    fn try_read_buf<B: MutBuf>(&mut self, buf: &mut B) -> Poll<usize, io::Error>;
}

impl<T: io::Read> TryRead for T {
    fn try_read(&mut self, buf: &mut [u8]) -> Poll<usize, io::Error> {
        match self.read(buf) {
            Ok(n) => Ok(Async::Ready(n)),
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => Ok(Async::NotReady),
            Err(e) => Err(e),
        }
    }

    fn try_read_buf<B: MutBuf>(&mut self, buf: &mut B) -> Poll<usize, io::Error> {
        match self.read_buf(buf) {
            Ok(n) => Ok(Async::Ready(n)),
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => Ok(Async::NotReady),
            Err(e) => Err(e),
        }
    }
}

/// A refinement of `std::io::Write` for reading from non-blocking sources.
///
/// When reading from a non-blocking source, receiving a
/// `ErrorKind::WouldBlock` is expected and not an error. A such, it is useful
/// to split out the would-block case from the other error cases.
pub trait TryWrite: io::Write {

    /// Write a buffer into this object, returning how many bytes were written.
    ///
    /// If the source is not able to perform the operation due to not being
    /// ready, `Ok(Async::NotReady)` is returned.
    ///
    /// Aside from the signature, behavior is identical to `std::io::Write`.
    /// For more details, read the `std::io::Write` documentation.
    fn try_write(&mut self, buf: &[u8]) -> Poll<usize, io::Error>;

    /// Write a `Buf` into this object, returning how many bytes were written.
    fn try_write_buf<B: Buf>(&mut self, buf: &mut B) -> Poll<usize, io::Error>;

    /// Try flushing the underlying IO
    fn try_flush(&mut self) -> Poll<(), io::Error>;
}

impl<T: io::Write> TryWrite for T {
    fn try_write(&mut self, buf: &[u8]) -> Poll<usize, io::Error> {
        match self.write(buf) {
            Ok(n) => Ok(Async::Ready(n)),
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => Ok(Async::NotReady),
            Err(e) => Err(e),
        }
    }

    fn try_write_buf<B: Buf>(&mut self, buf: &mut B) -> Poll<usize, io::Error> {
        match self.write_buf(buf) {
            Ok(n) => Ok(Async::Ready(n)),
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => Ok(Async::NotReady),
            Err(e) => Err(e),
        }
    }

    fn try_flush(&mut self) -> Poll<(), io::Error> {
        match self.flush() {
            Ok(()) => Ok(Async::Ready(())),
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => Ok(Async::NotReady),
            Err(e) => Err(e),
        }
    }
}
