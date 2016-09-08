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
mod multiplex;
mod server;

pub use self::server::Server;

use tokio_service::{Service};
use futures::{Async, Future};
use futures::stream::{Stream, Sender, Empty};
use take::Take;
use std::{fmt, cmp, io, ops};

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

/// Message sent and received from a multiplexed service
pub enum Message<T, B = Empty<(), ()>> {
    /// Has no associated streaming body
    WithoutBody(T),
    /// Has associated streaming body
    WithBody(T, B),
}

/// Error returned as an Error frame or an `io::Error` that occurerred during
/// normal processing of the Transport
pub enum Error<E> {
    /// Transport frame level error
    Transport(E),
    /// I/O level error
    Io(io::Error),
}

/// A specialization of `Service` supporting the requirements of server
/// pipelined services
///
/// `Service` should be implemented instead of this trait.
pub trait ServerService {
    /// Requests handled by the service.
    type Req;

    /// Responses given by the service.
    type Resp;

    /// Response body chunk
    type Body;

    /// Response body stream
    type BodyStream: Stream<Item = Self::Body, Error = Self::Error>;

    /// Errors produced by the service.
    type Error;

    /// The future response value.
    type Fut: Future<Item = Message<Self::Resp, Self::BodyStream>, Error = Self::Error>;

    /// Process the request and return the response asynchronously.
    fn call(&self, req: Self::Req) -> Self::Fut;
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
    fn read(&mut self) -> io::Result<Option<Frame<Self::Out, Self::BodyOut, Self::Error>>>;

    /// Tests to see if this I/O object may be writable.
    fn poll_write(&mut self) -> Async<()>;

    /// Write a message to the `Transport`
    fn write(&mut self, req: Frame<Self::In, Self::BodyIn, Self::Error>) -> io::Result<Option<()>>;

    /// Flush pending writes to the socket
    fn flush(&mut self) -> io::Result<Option<()>>;
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

    /// Create and return a new `Transport`
    fn new_transport(&self) -> io::Result<Self::Item>;
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
 * ===== impl Message =====
 *
 */

impl<T, B> Message<T, B> {
    /// If the `Message` value has an associated body stream, return it. The
    /// original `Message` value will then become a `WithoutBody` variant.
    pub fn take_body(&mut self) -> Option<B> {
        use std::ptr;

        // unfortunate that this is unsafe, but I think it is preferable to add
        // a little bit of unsafe code instead of adding a useless variant to
        // Message.
        unsafe {
            match ptr::read(self as *const Message<T, B>) {
                m @ Message::WithoutBody(..) => {
                    ptr::write(self as *mut Message<T, B>, m);
                    None
                }
                Message::WithBody(m, b) => {
                    ptr::write(self as *mut Message<T, B>, Message::WithoutBody(m));
                    Some(b)
                }
            }
        }
    }
}

impl<T, B> cmp::PartialEq<T> for Message<T, B>
    where T: cmp::PartialEq
{
    fn eq(&self, other: &T) -> bool {
        (**self).eq(other)
    }
}

impl<T, B> ops::Deref for Message<T, B> {
    type Target = T;

    fn deref(&self) -> &T {
        match *self {
            Message::WithoutBody(ref v) => v,
            Message::WithBody(ref v, _) => v,
        }
    }
}

impl<T, B> ops::DerefMut for Message<T, B> {
    fn deref_mut(&mut self) -> &mut T {
        match *self {
            Message::WithoutBody(ref mut v) => v,
            Message::WithBody(ref mut v, _) => v,
        }
    }
}

impl<T, B> fmt::Debug for Message<T, B>
    where T: fmt::Debug
{
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Message::WithoutBody(ref v) => write!(fmt, "Message::WithoutBody({:?})", v),
            Message::WithBody(ref v, _) => write!(fmt, "Message::WithBody({:?}, ...)", v),
        }
    }
}

/*
 *
 * ===== impl ServerService =====
 *
 */

impl<S, Resp, Body, BodyStream> ServerService for S
    where S: Service<Resp = Message<Resp, BodyStream>>,
          BodyStream: Stream<Item = Body, Error = S::Error>,
{
    type Req = S::Req;
    type Resp = Resp;
    type Body = Body;
    type BodyStream = BodyStream;
    type Error = S::Error;
    type Fut = S::Fut;

    fn call(&self, req: Self::Req) -> Self::Fut {
        Service::call(self, req)
    }
}

/*
 *
 * ===== impl Transport =====
 *
 */

impl<T, M1, M2, B1, B2, E> Transport for T
    where T: ::Transport<In = Frame<M1, B1, E>, Out = Frame<M2, B2, E>>,
{
    type In = M1;
    type BodyIn = B1;
    type Out = M2;
    type BodyOut = B2;
    type Error = E;

    fn poll_read(&mut self) -> Async<()> {
        ::Transport::poll_read(self)
    }

    fn read(&mut self) -> io::Result<Option<Frame<M2, B2, E>>> {
        ::Transport::read(self)
    }

    fn poll_write(&mut self) -> Async<()> {
        ::Transport::poll_write(self)
    }

    fn write(&mut self, req: Frame<M1, B1, E>) -> io::Result<Option<()>> {
        ::Transport::write(self, req)
    }

    fn flush(&mut self) -> io::Result<Option<()>> {
        ::Transport::flush(self)
    }
}

/*
 *
 * ===== impl NewTransport =====
 *
 */

impl<F, T> NewTransport for F
    where F: Fn() -> io::Result<T>,
          T: Transport,
{
    type In = T::In;
    type BodyIn = T::BodyIn;
    type Out = T::Out;
    type BodyOut = T::BodyOut;
    type Error = T::Error;
    type Item = T;

    fn new_transport(&self) -> io::Result<T> {
        self()
    }
}

impl<F, T> NewTransport for Take<F>
    where F: FnOnce() -> io::Result<T>,
          T: Transport,
{
    type In = T::In;
    type BodyIn = T::BodyIn;
    type Out = T::Out;
    type BodyOut = T::BodyOut;
    type Error = T::Error;
    type Item = T;

    fn new_transport(&self) -> io::Result<T> {
        self.take()()
    }
}

impl From<Error<io::Error>> for io::Error {
    fn from(err: Error<io::Error>) -> Self {
        match err {
            Error::Transport(e) |
            Error::Io(e) => e,
        }
    }
}
