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

pub use self::client::{connect, ClientHandle};
pub use self::server::Server;

use io::{Readiness};
use tcp::TcpStream;
use std::{fmt, io};

/// A pipelined protocol frame
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum Frame<T, E> {
    /// Either a request or a response
    Message(T),
    /// Error
    Error(E),
    /// Final frame sent in each transport direction
    Done,
}

/// Error returned as an Error frame or an io::Error that occurerred during
/// normal processing of the Transport
pub enum Error<E> {
    /// Transport frame level error
    Transport(E),
    /// I/O level error
    Io(io::Error),
}

/// A specialization of `io::Transport` supporting the requirements of
/// pipeline based protocols.
///
/// `io::Transport` should be implemented instead of this trait.
pub trait Transport: Readiness {
    /// Messages written to the transport
    type In: Send + 'static;

    /// Messages read from the transport
    type Out: Send + 'static;

    /// Errors
    type Error: Send + 'static; // TODO: rename

    /// Read a message from the `Transport`
    fn read(&mut self) -> io::Result<Option<Frame<Self::Out, Self::Error>>>;

    /// Write a message to the `Transport`
    fn write(&mut self, req: Frame<Self::In, Self::Error>) -> io::Result<Option<()>>;

    /// Flush pending writes to the socket
    fn flush(&mut self) -> io::Result<Option<()>>;
}

/// A specialization of `io::NewTransport` supporting the requirements of
/// pipeline based protocols.
///
/// `io::NewTransport` should be implemented instead of this trait.
pub trait NewTransport: Send + 'static {
    /// Messages written to the transport
    type In: Send + 'static;

    /// Messages read from the transport
    type Out: Send + 'static;

    /// Errors
    type Error: Send + 'static;

    /// Transport returned
    type Item: Transport<In = Self::In, Out = Self::Out, Error = Self::Error>;

    /// Create and return a new `Transport`
    fn new_transport(&self, socket: TcpStream) -> io::Result<Self::Item>;
}

impl<T, E> Frame<T, E> {
    /// Unwraps a frame, yielding the content of the `Message`.
    pub fn unwrap_msg(self) -> T where E: fmt::Debug {
        match self {
            Frame::Message(v) => v,
            Frame::Error(e) => panic!("called `Frame::unwrap_msg()` on an `Error` value: {:?}", e),
            Frame::Done => panic!("called `Frame::unwrap_msg()` on a `Done` value"),
        }
    }

    /// Unwraps a frame, yielding the content of the `Error`.
    pub fn unwrap_err(self) -> E where T: fmt::Debug {
        match self {
            Frame::Error(e) => e,
            Frame::Message(v) => panic!("called `Frame::unwrap_err()` on a `Message` value: {:?}", v),
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

impl<T, U, V, E> Transport for T
    where T: ::io::Transport<In = Frame<U, E>, Out = Frame<V, E>>,
          U: Send + 'static,
          V: Send + 'static,
          E: Send + 'static,
{
    type In = U;
    type Out = V;
    type Error = E;

    fn read(&mut self) -> io::Result<Option<Frame<V, E>>> {
        ::io::Transport::read(self)
    }

    fn write(&mut self, req: Frame<U, E>) -> io::Result<Option<()>> {
        ::io::Transport::write(self, req)
    }

    fn flush(&mut self) -> io::Result<Option<()>> {
        ::io::Transport::flush(self)
    }
}

impl<F, T> NewTransport for F
    where F: Fn(TcpStream) -> io::Result<T> + Send + 'static,
          T: Transport,
{
    type In = T::In;
    type Out = T::Out;
    type Error = T::Error;
    type Item = T;

    fn new_transport(&self, socket: TcpStream) -> io::Result<T> {
        self(socket)
    }
}

impl From<Error<io::Error>> for io::Error {
    fn from(err: Error<io::Error>) -> Self {
        match err {
            Error::Transport(e) => e,
            Error::Io(e) => e,
        }
    }
}
