//! Traits, helpers, and type definitions for Tokio I/O & non-blocking IO
//! functionality.
//!
//! # Readiness
//!
//! A source is **ready** when performing an operation has a high likelihood
//! (but not a guarantee) of successfully making progress.
//!
//! For example, a TCP socket is ready to perform a read operation when the
//! internal buffer has pending bytes. The same socket is ready to perform a
//! write when its write buffer is not full.
//!
//! The `Readiness` trait represents this concept. The `Readiness` trait also
//! acts as a marker trait to denotate types which are *Tokio aware*. A Tokio
//! aware type is able to observe usage and signal interest to the `Reactor`.
//!
//! In the case of `TcpStream`, the Tokio aware TCP socket, when
//! `TcpStream::try_read` is called and `Ok(None)` is returned, read interest
//! is automatically registered with the reactor an associated with the
//! currently running task. The logic being that, by trying to read from the
//! socket, the current task has indicated that read readiness is required for
//! the task to make progress. Once the TCP socket becomes read ready, the
//! task's `tick` function is called again, and this time a call to
//! `TcpStream::try_read` will (most likely) read some bytes OR result in an
//! error.
//!
//! # Transport
//!
//! A typed stream that may be read from and written to without blocking.
//!
//! A `Transport` does protocol specific serialization / deserialization. It
//! usually is composed of some lower level source, for example a TCP Stream
//! and reads / writes protocol frames.
//!
//! For example, imagine a protocol that is UTF-8 string based and framed with
//! new lines, aka each message is terminated by "\n". The transport would look
//! something like:
//!
//! ```rust,no_run
//! use tokio::io::{TryRead, TryWrite, Readiness, Transport};
//! use std::io;
//!
//! struct LineTransport<T> {
//!     source: T,
//!     rd: Vec<u8>,
//!     wr: Vec<u8>,
//! }
//!
//! impl<T: TryRead + TryWrite + Readiness> Transport for LineTransport<T> {
//!     type In = String;
//!     type Out = String;
//!
//!     fn read(&mut self) -> io::Result<Option<String>> {
//!         loop {
//!             // First, try to parse a new line from what was buffered
//!             if let Some(line) = parse_new_line(&mut self.rd) {
//!                 return Ok(Some(line));
//!             }
//!         }
//!     }
//!
//!     fn write(&mut self, req: String) -> io::Result<Option<()>> {
//!         // Append the message to the buffer
//!         self.wr.append(&mut req.into_bytes());
//!         self.flush()
//!     }
//!
//!     fn flush(&mut self) -> io::Result<Option<()>> {
//!         while !self.wr.is_empty() {
//!             match try!(self.source.try_write(&mut self.wr)) {
//!                 Some(n) => shift(&mut self.wr, n),
//!                 None => return Ok(None),
//!             }
//!         }
//!
//!         Ok(Some(()))
//!     }
//! }
//!
//! impl<T: Readiness> Readiness for LineTransport<T> {
//!     fn is_readable(&self) -> bool {
//!         self.rd.contains(&b'\n') || self.source.is_readable()
//!     }
//!
//!     fn is_writable(&self) -> bool {
//!         // The source is always writable since we don't cap the write
//!         // buffer size. Not an ideal production behavior...
//!         true
//!     }
//! }
//!
//! fn parse_new_line(buf: &mut Vec<u8>) -> Option<String> {
//!     // Search for "\n", if found, read everything from the start of the
//!     // buffer to the newline as a string then shift all bytes after the
//!     // newline to the beginning of the buffer.
//!     unimplemented!();
//! }
//!
//! fn shift(buf: &mut Vec<u8>, n: usize) {
//!     // Drop the first `n` bytes in the buffer and move all bytes after that
//!     // point to the front of the buffer.
//! }
//! ```

mod ready;
mod transport;

pub use self::ready::{Readiness, Ready};
pub use self::transport::Transport;
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
    /// any bytes to read, `Ok(None)` is returned.
    ///
    /// Aside from the signature, behavior is identical to `std::io::Read`. For
    /// more details, read the `std::io::Read` documentation.
    fn try_read(&mut self, buf: &mut [u8]) -> io::Result<Option<usize>>;
}

impl<T: io::Read> TryRead for T {
    fn try_read(&mut self, buf: &mut [u8]) -> io::Result<Option<usize>> {
        match self.read(buf) {
            Ok(n) => Ok(Some(n)),
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => Ok(None),
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
    /// ready, `Ok(None)` is returned.
    ///
    /// Aside from the signature, behavior is identical to `std::io::Write`.
    /// For more details, read the `std::io::Write` documentation.
    fn try_write(&mut self, buf: &[u8]) -> io::Result<Option<usize>>;
}

impl<T: io::Write> TryWrite for T {
    fn try_write(&mut self, buf: &[u8]) -> io::Result<Option<usize>> {
        match self.write(buf) {
            Ok(n) => Ok(Some(n)),
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => Ok(None),
            Err(e) => Err(e),
        }
    }
}
