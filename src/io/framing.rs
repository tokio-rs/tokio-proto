#![allow(warnings)]

use io::{Readiness, Stream, Transport, TryRead, TryWrite};
use bytes::{alloc, MutBuf, BlockBuf, Source};
use std::io;

/// Transport handling frame encoding and decoding.
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
    fn done(&mut self, buf: &mut BlockBuf) -> Option<Self::Out> {
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
    where T: Stream,
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

impl<T, P, S> Transport for Framed<T, P, S>
    where T: Stream,
          P: Parse,
          S: Serialize,
{
    type In = S::In;
    type Out = P::Out;

    fn read(&mut self) -> io::Result<Option<Self::Out>> {
        loop {
            // If the read buffer has any pending data, then it could be
            // possible that `parse` will return a new frame. We leave it to
            // the parser to optimize detecting that more data is required.
            if !self.rd.is_empty()  {
                trace!("read buffer has data");
                if let Some(frame) = self.parse.parse(&mut self.rd) {
                    trace!("frame parsed from buffer");
                    self.is_readable = true;
                    return Ok(Some(frame));
                }

                self.is_readable = false;
            }

            assert!(self.rd.remaining() > 0);

            // Otherwise, try to read more data and try again
            match try!(self.upstream.try_read_buf(&mut self.rd)) {
                Some(0) => {
                    trace!("read 0 bytes");
                    return Ok(self.parse.done(&mut self.rd));
                }
                Some(_) => {}
                None => {
                    trace!("upstream Transport::read returned would-block");
                    return Ok(None);
                }
            }
        }
    }

    fn write(&mut self, msg: Self::In) -> io::Result<Option<()>> {
        if !self.is_writable() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "transport not currently writable"));
        }

        // Serialize the msg
        self.serialize.serialize(msg, &mut self.wr);

        // Writing to the socket until flush. This allows buffering up more
        // data and reducing the number of syscalls made.
        //
        // TODO: A better solution may be to wait until a block in the
        // `BlockBuf` is full and then flush that block immediately.
        Ok(None)
    }

    fn flush(&mut self) -> io::Result<Option<()>> {
        // Try flushing the underlying IO
        let _ = try!(self.upstream.try_flush());

        trace!("flushing framed transport");

        loop {
            if self.wr.is_empty() {
                trace!("framed transport flushed");
                return Ok(Some(()));
            }

            trace!("writing; remaining={:?}", self.wr.len());

            match self.upstream.try_write_buf(&mut self.wr.buf()) {
                Ok(Some(n)) => self.wr.drop(n),
                Ok(None) => return Ok(None),
                Err(e) => {
                    trace!("framed transport flush error; err={:?}", e);
                    return Err(e);
                }
            }
        }
    }
}

impl<T, P, S> Readiness for Framed<T, P, S>
    where T: Stream
{
    fn is_readable(&self) -> bool {
        self.is_readable || self.upstream.is_readable()
    }

    fn is_writable(&self) -> bool {
        // Always accept writes and let the write buffer grow
        //
        // TODO: This may not be the best option for robustness, but for now it
        // makes the microbenchmarks happy.
        true
    }

}
