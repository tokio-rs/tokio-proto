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

pub use self::multiplex::{Multiplex, Dispatch};
pub use self::client::connect;
pub use self::server::Server;

use {Message};
use tokio_core::io::FramedIo;
use tokio_service::{Service};
use futures::{Async, Future, IntoFuture, Poll};
use futures::stream::{Stream, Sender};
use take::Take;
use std::{fmt, io};

/// Identifies a request / response thread
pub type RequestId = u64;

/// A multiplexed protocol frame
pub enum Frame<T, B, E> {
    /// Either a request or a response
    Message(RequestId, T),
    /// Returned by `Transport::read` when a streaming body will follow.
    /// Subsequent body frames with a matching `RequestId` will be proxied to
    /// the provided `Sender`.
    ///
    /// Calling `Transport::write` with Frame::MessageWithBody is an error.
    MessageWithBody(RequestId, T, Sender<B, E>),
    /// Body frame. None indicates that the body is done streaming.
    Body(RequestId, Option<B>),
    /// Error
    Error(RequestId, E),
    /// Final frame sent in each transport direction
    Done,
}

/// A specialization of `Service` supporting the requirements of server
/// pipelined services
///
/// `Service` should be implemented instead of this trait.
pub trait ServerService {
    /// Requests handled by the service.
    type Request;

    /// Responses given by the service.
    type Response;

    /// Response body chunk
    type Body;

    /// Response body stream
    type BodyStream: Stream<Item = Self::Body, Error = Self::Error>;

    /// Errors produced by the service.
    type Error;

    /// The future response value.
    type Future: Future<Item = Message<Self::Response, Self::BodyStream>, Error = Self::Error>;

    /// Process the request and return the response asynchronously.
    fn call(&self, req: Self::Request) -> Self::Future;
}

/// A specialization of `io::Transport` supporting the requirements of
/// pipeline based protocols.
///
/// `io::Transport` should be implemented instead of this trait.
pub trait Transport {
    /// Messages written to the transport
    type In;

    /// Inbound body frame
    type BodyIn;

    /// Messages read from the transport
    type Out;

    /// Outbound body frame
    type BodyOut;

    /// Transport error
    type Error;

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

/// A specialization of `io::NewTransport` supporting the requirements of
/// pipeline based protocols.
///
/// `io::NewTransport` should be implemented instead of this trait.
pub trait NewTransport {
    /// Messages written to the transport
    type In;

    /// Inbound streaming body
    type BodyIn;

    /// Messages read from the transport
    type Out;

    /// Outbound streaming body
    type BodyOut;

    /// Errors
    type Error;

    /// Transport returned
    type Item: Transport<In = Self::In,
                     BodyIn = Self::BodyIn,
                        Out = Self::Out,
                    BodyOut = Self::BodyOut,
                      Error = Self::Error>;

    /// The Future transport
    type Future: Future<Item = Self::Item, Error = io::Error>;

    /// Create and return a new `Transport`
    fn new_transport(&self) -> Self::Future;
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
            Frame::Message(id, _) => Some(id),
            Frame::MessageWithBody(id, _, _) => Some(id),
            Frame::Body(id, _) => Some(id),
            Frame::Error(id, _) => Some(id),
            Frame::Done => None,
        }
    }

    /// Unwraps a frame, yielding the content of the `Message`.
    pub fn unwrap_msg(self) -> T {
        match self {
            Frame::Message(_, v) => v,
            Frame::MessageWithBody(_, v, _) => v,
            Frame::Body(..) => panic!("called `Frame::unwrap_msg()` on a `Body` value"),
            Frame::Error(..) => panic!("called `Frame::unwrap_msg()` on an `Error` value"),
            Frame::Done => panic!("called `Frame::unwrap_msg()` on a `Done` value"),
        }
    }

    /// Unwraps a frame, yielding the content of the `Body`.
    pub fn unwrap_body(self) -> Option<B> {
        match self {
            Frame::Body(_, v) => v,
            Frame::Message(..) => panic!("called `Frame::unwrap_body()` on a `Message` value"),
            Frame::MessageWithBody(..) => panic!("called `Frame::unwrap_body()` on a `MessageWithBody` value"),
            Frame::Error(..) => panic!("called `Frame::unwrap_body()` on an `Error` value"),
            Frame::Done => panic!("called `Frame::unwrap_body()` on a `Done` value"),
        }
    }

    /// Unwraps a frame, yielding the content of the `Error`.
    pub fn unwrap_err(self) -> E {
        match self {
            Frame::Error(_, e) => e,
            Frame::Body(..) => panic!("called `Frame::unwrap_err()` on a `Body` value"),
            Frame::Message(..) => panic!("called `Frame::unwrap_err()` on a `Message` value"),
            Frame::MessageWithBody(..) => panic!("called `Frame::unwrap_err()` on a `MessageWithBody` value"),
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
            Frame::Message(ref id, ref v) => write!(fmt, "Frame::Message({:?}, {:?})", id, v),
            Frame::MessageWithBody(ref id, ref v, _) => write!(fmt, "Frame::MessageWithBody({:?}, {:?}, Sender)", id, v),
            Frame::Body(ref id, ref v) => write!(fmt, "Frame::Body({:?}, {:?})", id, v),
            Frame::Error(ref id, ref v) => write!(fmt, "Frame::Error({:?}, {:?})", id, v),
            Frame::Done => write!(fmt, "Frame::Done"),
        }
    }
}

/*
 *
 * ===== impl ServerService =====
 *
 */

impl<S, Response, Body, BodyStream> ServerService for S
    where S: Service<Response = Message<Response, BodyStream>>,
          BodyStream: Stream<Item = Body, Error = S::Error>,
{
    type Request = S::Request;
    type Response = Response;
    type Body = Body;
    type BodyStream = BodyStream;
    type Error = S::Error;
    type Future = S::Future;

    fn call(&self, req: Self::Request) -> Self::Future {
        Service::call(self, req)
    }
}

/*
 *
 * ===== impl Transport =====
 *
 */

impl<T, M1, M2, B1, B2, E> Transport for T
    where T: FramedIo<In = Frame<M1, B1, E>, Out = Frame<M2, B2, E>>,
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

/*
 *
 * ===== impl NewTransport =====
 *
 */

impl<F, R, T> NewTransport for F
    where F: Fn() -> R,
          R: IntoFuture<Item = T, Error = io::Error>,
          T: Transport,
{
    type In = T::In;
    type BodyIn = T::BodyIn;
    type Out = T::Out;
    type BodyOut = T::BodyOut;
    type Error = T::Error;
    type Item = T;
    type Future = R::Future;

    fn new_transport(&self) -> Self::Future {
        self().into_future()
    }
}

impl<F, R, T> NewTransport for Take<F>
    where F: FnOnce() -> R,
          R: IntoFuture<Item = T, Error = io::Error>,
          T: Transport,
{
    type In = T::In;
    type BodyIn = T::BodyIn;
    type Out = T::Out;
    type BodyOut = T::BodyOut;
    type Error = T::Error;
    type Item = T;
    type Future = R::Future;

    fn new_transport(&self) -> Self::Future {
        self.take()().into_future()
    }
}
