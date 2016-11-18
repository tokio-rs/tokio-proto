//! A key part of any protocol is its **transport**, which is the way that it
//! sends and received *frames* on its connection. For simple protocols (RPC,
//! pipelined), these frames correspond directly to complete requests and
//! responses. For more complicated protocols, they carry additional metadata,
//! and may only be one part of a request or response body.
//!
//! Transports are defined by implementing the `Transport` trait. The
//! `transport::CodecTransport` type can be used to wrap a `Codec` (from
//! `tokio-core`), which is a simple way to build a transport.

use std::io;
use futures::{Future, Poll, StartSend};

/// A transport provides a way of reading and writing frames from an underlying
/// I/O object.
///
/// The trait just joins together a `Stream` for reading frames and a `Sink` for
/// writing them. At the moment, it also provides a method
pub trait Transport<T>: 'static + Sized {
    /// Frames read out of the transport.
    type ReadFrame;

    /// Frames written to the transport.
    type WriteFrame;

    /// A future for setting up a new transport.
    type Bind: Future<Item = Self, Error = io::Error> + 'static;

    /// Set up a transport for the given I/O object, with default configuration.
    fn bind(io: T) -> Self::Bind where Self: Sized;

    /// Attempt to pull out the next frame from this transport, returning `None` if
    /// the read side of the transport has terminated.
    ///
    /// This method is an inlined version of `Stream::poll`; see the
    /// documentation there for more details.
    fn poll(&mut self) -> Poll<Option<Self::ReadFrame>, io::Error>;

    /// Starts sending a frame on the transport.
    ///
    /// This method is an inlined version of `Sink::start_send`; see the
    /// documentation there for more details.
    fn start_send(&mut self, frame: Self::WriteFrame) -> StartSend<Self::WriteFrame, io::Error>;

    /// Make progress on pending writes to the transport, determining whether
    /// they've completed.
    ///
    /// This method is an inlined version of `Sink::poll_complete`; see the
    /// documentation there for more details.
    fn poll_complete(&mut self) -> Poll<(), io::Error>;

    /// Allow the transport to do miscellaneous work (e.g., sending ping-pong
    /// messages) that is not directly connected to sending or receiving frames.
    ///
    /// This method should be called every time the task using the transport is
    /// executing.
    fn tick(&mut self) {}
}

pub use tokio_core::io::Framed as CodecTransport;

/*
impl<T, A> Transport<T> for A where
    A: Stream<Error = io::Error>,
    A: Sink<SinkError = io::Error>,
    A: FutureFrom<T>,
{
    type ReadFrame = A::Item;
    type WriteFrame = A::SinkItem;
    type Bind = A::Future;

    fn bind(io: T) -> Self::Bind {
        FutureFrom::future_from(io)
    }

    fn poll(&mut self) -> Poll<Option<Self::ReadFrame>, io::Error> {
        Stream::poll(self)
    }

    fn start_send(&mut self, frame: Self::WriteFrame) -> StartSend<Self::WriteFrame, io::Error> {
        Sink::start_send(self, frame)
    }

    fn poll_complete(&mut self) -> Poll<(), io::Error> {
        Sink::poll_complete(self)
    }
}
*/
