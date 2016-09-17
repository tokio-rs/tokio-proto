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

pub use self::client::{connect, Client, Connecting};
pub use self::server::Server;

use tokio_core::io::FramedIo;
use tokio_service::Service;
use futures::{Async, Future, IntoFuture, Poll};
use futures::stream::{Stream, Sender};
use take::Take;
use std::{cmp, fmt, io, ops};

/// A pipelined protocol frame
pub enum Frame<T, B, E> {
    /// Either a request or a response
    Message(T),
    /// Returned by `Transport::read` when a streaming body will follow.
    /// Subsequent body frames will be proxied to the provided `Sender`.
    ///
    /// Calling `Transport::write` with Frame::MessageWithBody is an error.
    MessageWithBody(T, Sender<B, E>),
    /// Body frame. None indicates that the body is done streaming.
    Body(Option<B>),
    /// Error
    Error(E),
    /// Final frame sent in each transport direction
    Done,
}

/// Message sent and received from a pipeline service
pub enum Message<T, B> {
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
    /// Unwraps a frame, yielding the content of the `Message`.
    pub fn unwrap_msg(self) -> T {
        match self {
            Frame::Message(v) => v,
            Frame::MessageWithBody(v, _) => v,
            Frame::Body(..) => panic!("called `Frame::unwrap_msg()` on a `Body` value"),
            Frame::Error(..) => panic!("called `Frame::unwrap_msg()` on an `Error` value"),
            Frame::Done => panic!("called `Frame::unwrap_msg()` on a `Done` value"),
        }
    }

    /// Unwraps a frame, yielding the content of the `Body`.
    pub fn unwrap_body(self) -> Option<B> {
        match self {
            Frame::Body(v) => v,
            Frame::Message(..) => panic!("called `Frame::unwrap_body()` on a `Message` value"),
            Frame::MessageWithBody(..) => panic!("called `Frame::unwrap_body()` on a `MessageWithBody` value"),
            Frame::Error(..) => panic!("called `Frame::unwrap_body()` on an `Error` value"),
            Frame::Done => panic!("called `Frame::unwrap_body()` on a `Done` value"),
        }
    }

    /// Unwraps a frame, yielding the content of the `Error`.
    pub fn unwrap_err(self) -> E {
        match self {
            Frame::Error(e) => e,
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
            Frame::Message(ref v) => write!(fmt, "Frame::Message({:?})", v),
            Frame::MessageWithBody(ref v, _) => write!(fmt, "Frame::MessageWithBody({:?}, Sender)", v),
            Frame::Body(ref v) => write!(fmt, "Frame::Body({:?})", v),
            Frame::Error(ref v) => write!(fmt, "Frame::Error({:?})", v),
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

impl From<Error<io::Error>> for io::Error {
    fn from(err: Error<io::Error>) -> Self {
        match err {
            Error::Transport(e) |
            Error::Io(e) => e,
        }
    }
}
