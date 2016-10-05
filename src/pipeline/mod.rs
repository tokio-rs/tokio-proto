//! A dispatcher for pipelining protocols
//!
//! This module contains reusable components for quickly implementing clients
//! and servers for pipeline based protocols.
//!
//! # Pipelining
//!
//! Protocol pipelining is a technique in which multiple requests are written
//! out to a single destination without waiting for their corresponding
//! responses. Pipelining is used in a multitude of different protocols such as
//! HTTP/1.1 and Redis in order to increase throughput on a single connection.
//!
//! Pipelining with the max number of in-flight requests set to 1 implies that
//! for each request, the response must be received before sending another
//! request on the same connection.
//!
//! Another protocol dispatching strategy is multiplexing (which will be
//! included in Tokio soon).
//!
//! # Usage
//!
//! Both the server and client pipeline dispatchers take a generic `Transport`
//! that reads and writes `Frame` messages. It operates on the transport
//! following the rules of pipelining as described above and exposes the
//! protocol using a `Service`.

mod client;
mod server;
mod pipeline;

pub use self::pipeline::{Pipeline, PipelineMessage, Dispatch};
pub use self::server::Server;
pub use self::client::connect;

use {Error};
use tokio_core::io::FramedIo;
use futures::{Async, Poll};
use std::{fmt, io};

/// A pipelined protocol frame
pub enum Frame<T, B, E> {
    /// Either a request or a response
    Message {
        /// The message value
        message: T,
        /// Set to true when body frames will follow with the same request ID.
        body: bool,
    },
    /// Body frame. None indicates that the body is done streaming.
    Body {
        /// Body chunk. Setting to `None` indicates that the body is done
        /// streaming and there will be no further body frames sent with the
        /// given request ID.
        chunk: Option<B>,
    },
    /// Error
    Error {
        /// Error value
        error: E,
    },
    /// Final frame sent in each transport direction
    Done,
}

/// A specialization of `io::Transport` supporting the requirements of
/// pipeline based protocols.
///
/// `io::Transport` should be implemented instead of this trait.
pub trait Transport: 'static {
    /// Messages written to the transport
    type In: 'static;

    /// Inbound body frame
    type BodyIn: 'static;

    /// Messages read from the transport
    type Out: 'static;

    /// Outbound body frame
    type BodyOut: 'static;

    /// Transport error
    type Error: From<Error<Self::Error>> + 'static;

    /// Tests to see if this Transport may be readable.
    fn poll_read(&mut self) -> Async<()>;

    /// Read a message from the `Transport`
    fn read(&mut self) -> Poll<Frame<Self::Out, Self::BodyOut, Self::Error>, io::Error>;

    /// Tests to see if this I/O object may be writable.
    fn poll_write(&mut self) -> Async<()>;

    /// Write a message to the `Transport`
    fn write(&mut self, req: Frame<Self::In, Self::BodyIn, Self::Error>) -> Poll<(), io::Error>;

    /// Flush pending writes to the socket
    fn flush(&mut self) -> Poll<(), io::Error>;
}

/*
 *
 * ===== impl Frame =====
 *
 */

impl<T, B, E> Frame<T, B, E> {
    /// Unwraps a frame, yielding the content of the `Message`.
    pub fn unwrap_msg(self) -> T {
        match self {
            Frame::Message { message, .. } => message,
            Frame::Body { .. } => panic!("called `Frame::unwrap_msg()` on a `Body` value"),
            Frame::Error { .. } => panic!("called `Frame::unwrap_msg()` on an `Error` value"),
            Frame::Done => panic!("called `Frame::unwrap_msg()` on a `Done` value"),
        }
    }

    /// Unwraps a frame, yielding the content of the `Body`.
    pub fn unwrap_body(self) -> Option<B> {
        match self {
            Frame::Body { chunk } => chunk,
            Frame::Message { .. } => panic!("called `Frame::unwrap_body()` on a `Message` value"),
            Frame::Error { .. } => panic!("called `Frame::unwrap_body()` on an `Error` value"),
            Frame::Done => panic!("called `Frame::unwrap_body()` on a `Done` value"),
        }
    }

    /// Unwraps a frame, yielding the content of the `Error`.
    pub fn unwrap_err(self) -> E {
        match self {
            Frame::Error { error } => error,
            Frame::Body { .. } => panic!("called `Frame::unwrap_err()` on a `Body` value"),
            Frame::Message { .. } => panic!("called `Frame::unwrap_err()` on a `Message` value"),
            Frame::Done => panic!("called `Frame::unwrap_message()` on a `Done` value"),
        }
    }

    /// Returns true if the frame is `Frame::Done`
    pub fn is_done(&self) -> bool {
        match *self {
            Frame::Done => true,
            _ => false,
        }
    }
}

impl<T, B, E> fmt::Debug for Frame<T, B, E>
    where T: fmt::Debug,
          E: fmt::Debug,
          B: fmt::Debug,
{
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Frame::Message { ref message, body } => {
                fmt.debug_struct("Frame::Message")
                    .field("message", message)
                    .field("body", &body)
                    .finish()
            }
            Frame::Body { ref chunk } => {
                fmt.debug_struct("Frame::Body")
                    .field("chunk", chunk)
                    .finish()
            }
            Frame::Error { ref error } => {
                fmt.debug_struct("Frame::Error")
                    .field("error", error)
                    .finish()
            },
            Frame::Done => write!(fmt, "Frame::Done"),
        }
    }
}

/*
 *
 * ===== impl Transport =====
 *
 */

impl<T, M1, M2, B1, B2, E> Transport for T
    where T: FramedIo<In = Frame<M1, B1, E>, Out = Frame<M2, B2, E>> + 'static,
          E: From<Error<E>> + 'static,
          M1: 'static,
          M2: 'static,
          B1: 'static,
          B2: 'static,
{
    type In = M1;
    type BodyIn = B1;
    type Out = M2;
    type BodyOut = B2;
    type Error = E;

    fn poll_read(&mut self) -> Async<()> {
        FramedIo::poll_read(self)
    }

    fn read(&mut self) -> Poll<Frame<M2, B2, E>, io::Error> {
        FramedIo::read(self)
    }

    fn poll_write(&mut self) -> Async<()> {
        FramedIo::poll_write(self)
    }

    fn write(&mut self, req: Frame<M1, B1, E>) -> Poll<(), io::Error> {
        FramedIo::write(self, req)
    }

    fn flush(&mut self) -> Poll<(), io::Error> {
        FramedIo::flush(self)
    }
}

impl<M1, M2, B1, B2, E> Transport for Box<Transport<In = M1, Out = M2, BodyIn = B1, BodyOut = B2, Error = E>>
    where E: From<Error<E>> + 'static,
          M1: 'static,
          M2: 'static,
          B1: 'static,
          B2: 'static,
{
    type In = M1;
    type BodyIn = B1;
    type Out = M2;
    type BodyOut = B2;
    type Error = E;

    fn poll_read(&mut self) -> Async<()> {
        (**self).poll_read()
    }

    fn read(&mut self) -> Poll<Frame<M2, B2, E>, io::Error> {
        (**self).read()
    }

    fn poll_write(&mut self) -> Async<()> {
        (**self).poll_write()
    }

    fn write(&mut self, req: Frame<M1, B1, E>) -> Poll<(), io::Error> {
        (**self).write(req)
    }

    fn flush(&mut self) -> Poll<(), io::Error> {
        (**self).flush()
    }
}

impl<M1, M2, B1, B2, E> Transport for Box<Transport<In = M1, Out = M2, BodyIn = B1, BodyOut = B2, Error = E> + Send>
    where E: From<Error<E>> + 'static,
          M1: 'static,
          M2: 'static,
          B1: 'static,
          B2: 'static,
{
    type In = M1;
    type BodyIn = B1;
    type Out = M2;
    type BodyOut = B2;
    type Error = E;

    fn poll_read(&mut self) -> Async<()> {
        (**self).poll_read()
    }

    fn read(&mut self) -> Poll<Frame<M2, B2, E>, io::Error> {
        (**self).read()
    }

    fn poll_write(&mut self) -> Async<()> {
        (**self).poll_write()
    }

    fn write(&mut self, req: Frame<M1, B1, E>) -> Poll<(), io::Error> {
        (**self).write(req)
    }

    fn flush(&mut self) -> Poll<(), io::Error> {
        (**self).flush()
    }
}
