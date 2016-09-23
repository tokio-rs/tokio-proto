use futures::{Async, Poll};
use tokio_core::io::{Io, FramedIo};
use io::{TryRead, TryWrite};
use bytes::{MutBuf};
use bytes::buf::{BlockBuf};
use std::io;

/// FramedIo handling frame encoding and decoding.
pub struct Framed<T, P, S> {
    upstream: T,
    parse: P,
    serialize: S,
    // Set to true until `parse` returns `None`
    is_readable: bool,
    // Read buffer
    rd: BlockBuf,
    // Write buffer
    wr: BlockBuf,
}

/// Parses frames out of a `BlockBuf`
pub trait Parse {

    /// Parse result
    type Out;

    /// Optionally parse a frame from the given buffer.
    fn parse(&mut self, buf: &mut BlockBuf) -> Option<Self::Out>;

    /// Called when there are no more inbound bytes
    fn done(&mut self, _buf: &mut BlockBuf) -> Option<Self::Out> {
        None
    }
}

/// Serialize frames into a `BlockBuf`
pub trait Serialize {

    /// Type to serialize
    type In;

    /// Serialize the frame into the `BlockBuf`
    fn serialize(&mut self, msg: Self::In, buf: &mut BlockBuf);
}

impl<T, P, S> Framed<T, P, S>
    where T: Io,
          P: Parse,
          S: Serialize,
{
    /// Create a new `Framed`
    pub fn new(upstream: T,
               parse: P,
               serialize: S,
               rd: BlockBuf,
               wr: BlockBuf) -> Framed<T, P, S> {

        trace!("creating new framed transport");
        Framed {
            upstream: upstream,
            parse: parse,
            serialize: serialize,
            is_readable: false,
            rd: rd,
            wr: wr,
        }
    }
}

impl<T, P, S> FramedIo for Framed<T, P, S>
    where T: Io,
          P: Parse,
          S: Serialize,
{
    type In = S::In;
    type Out = P::Out;

    fn poll_read(&mut self) -> Async<()> {
        if self.is_readable || self.upstream.poll_read().is_ready() {
            Async::Ready(())
        } else {
            Async::NotReady
        }
    }

    fn read(&mut self) -> Poll<Self::Out, io::Error> {
        loop {
            // If the read buffer has any pending data, then it could be
            // possible that `parse` will return a new frame. We leave it to
            // the parser to optimize detecting that more data is required.
            if !self.rd.is_empty()  {
                trace!("read buffer has data");
                if let Some(frame) = self.parse.parse(&mut self.rd) {
                    trace!("frame parsed from buffer");
                    self.is_readable = true;
                    return Ok(Async::Ready(frame));
                }

                self.is_readable = false;
            }

            assert!(self.rd.remaining() > 0);

            // Otherwise, try to read more data and try again
            match try!(self.upstream.try_read_buf(&mut self.rd)) {
                Async::Ready(0) => {
                    trace!("read 0 bytes");

                    if let Some(v) = self.parse.done(&mut self.rd) {
                        return Ok(Async::Ready(v));
                    }

                    return Ok(Async::NotReady);
                }
                Async::Ready(_) => {}
                Async::NotReady => {
                    trace!("upstream Transport::read returned would-block");
                    return Ok(Async::NotReady);
                }
            }
        }
    }

    fn poll_write(&mut self) -> Async<()> {
        // Always accept writes and let the write buffer grow
        //
        // TODO: This may not be the best option for robustness, but for now it
        // makes the microbenchmarks happy.
        Async::Ready(())
    }

    fn write(&mut self, msg: Self::In) -> Poll<(), io::Error> {
        if !self.poll_write().is_ready() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "transport not currently writable"));
        }

        // Serialize the msg
        self.serialize.serialize(msg, &mut self.wr);

        // Writing to the socket until flush. This allows buffering up more
        // data and reducing the number of syscalls made.
        //
        // TODO: A better solution may be to wait until a block in the
        // `BlockBuf` is full and then flush that block immediately.
        Ok(Async::NotReady)
    }

    fn flush(&mut self) -> Poll<(), io::Error> {
        // Try flushing the underlying IO
        let _ = try!(self.upstream.try_flush());

        trace!("flushing framed transport");

        loop {
            if self.wr.is_empty() {
                trace!("framed transport flushed");
                return Ok(Async::Ready(()));
            }

            trace!("writing; remaining={:?}", self.wr.len());

            match self.upstream.try_write_buf(&mut self.wr.buf()) {
                Ok(Async::Ready(n)) => self.wr.drop(n),
                Ok(Async::NotReady) => return Ok(Async::NotReady),
                Err(e) => {
                    trace!("framed transport flush error; err={:?}", e);
                    return Err(e);
                }
            }
        }
    }
}
