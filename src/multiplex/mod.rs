//! Dispatch for multiplexed protocols
//!
//! This module contains reusable components for quickly implementing clients
//! and servers for multiplex based protocols.
//!
//! ## Multiplexing
//!
//! Multiplexing allows multiple request / response transactions to be inflight
//! concurrently while sharing the same underlying transport. This is usually
//! done by splitting the protocol into frames and assigning a request ID to
//! each frame.
//!
//! ## Considerations
//!
//! There are some difficulties with implementing back pressure in the case
//! that the wire protocol does not support a means by which backpressure can
//! be signaled to the peer.
//!
//! The problem is that, the transport is no longer read from while waiting for
//! the current frames to be processed, it is possible that the processing
//! logic is blocked waiting for another frame that is currently pending on the
//! socket.
//!
//! To deal with this, once the connection level frame buffer is filled, a
//! timeout is set. If no further frames are able to be read before the timeout
//! expires, then the connection is killed.
//!
//! ## Current status
//!
//! As of now, the implementation only supports multiplexed requests &
//! responses without streaming bodies.

mod frame_buf;
mod client;
mod multiplex;
mod server;

pub use self::multiplex::{Multiplex, MultiplexMessage, Dispatch};
pub use self::client::connect;
pub use self::server::Server;

use {Error};
use tokio_core::io::FramedIo;
use futures::{Async, Poll};
use std::{fmt, io};

/// Identifies a request / response thread
pub type RequestId = u64;

/// A multiplexed protocol frame
pub enum Frame<T, B, E> {
    /// Either a request or a response.
    Message {
        /// Message exchange identifier
        id: RequestId,
        /// The message value
        message: T,
        /// Set to true when body frames will follow with the same request ID.
        body: bool,
    },
    /// Body frame.
    Body {
        /// Message exchange identifier
        id: RequestId,
        /// Body chunk. Setting to `None` indicates that the body is done
        /// streaming and there will be no further body frames sent with the
        /// given request ID.
        chunk: Option<B>,
    },
    /// Error
    Error {
        /// Message exchange identifier
        id: RequestId,
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
    /// Return the request ID associated with the frame.
    pub fn request_id(&self) -> Option<RequestId> {
        match *self {
            Frame::Message { id, .. } => Some(id),
            Frame::Body { id, .. } => Some(id),
            Frame::Error { id, .. } => Some(id),
            Frame::Done => None,
        }
    }

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
            Frame::Body { chunk, .. } => chunk,
            Frame::Message { .. } => panic!("called `Frame::unwrap_body()` on a `Message` value"),
            Frame::Error { .. } => panic!("called `Frame::unwrap_body()` on an `Error` value"),
            Frame::Done => panic!("called `Frame::unwrap_body()` on a `Done` value"),
        }
    }

    /// Unwraps a frame, yielding the content of the `Error`.
    pub fn unwrap_err(self) -> E {
        match self {
            Frame::Error { error, .. } => error,
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
            Frame::Message { id, ref message, body } => {
                fmt.debug_struct("Frame::Message")
                    .field("id", &id)
                    .field("message", message)
                    .field("body", &body)
                    .finish()
            }
            Frame::Body { id, ref chunk } => {
                fmt.debug_struct("Frame::Body")
                    .field("id", &id)
                    .field("chunk", chunk)
                    .finish()
            }
            Frame::Error { ref id, ref error } => {
                fmt.debug_struct("Frame::Error")
                    .field("id", &id)
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
