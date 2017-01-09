//! Pipelined, multiplexed protocols.
//!
//! See the crate-level docs for an overview.

use std::{io, u64};
use futures::{Stream, Sink, Async};
use tokio_core::io::{Io, Framed, Codec};

mod frame_buf;

mod client;
pub use self::client::ClientProto;

mod server;
pub use self::server::ServerProto;

mod frame;
pub use self::frame::Frame;


pub mod advanced;

/// Identifies a request / response thread
pub type RequestId = u64;

/// A marker used to flag protocols as being streaming and multiplexed.
///
/// This is an implementation detail; to actually implement a protocol,
/// implement the `ClientProto` or `ServerProto` traits in this module.
pub struct StreamingMultiplex<B>(B);

/// Multiplex configuration options
#[derive(Debug, Copy, Clone)]
pub struct MultiplexConfig {
    /// Maximum number of in-flight requests
    max_in_flight: usize,
    max_response_displacement: u64,
    max_buffered_frames: usize,
}

impl MultiplexConfig {
    /// Set the maximum number of requests to process concurrently
    ///
    /// Default: 100
    pub fn max_in_flight(&mut self, val: usize) -> &mut Self {
        self.max_in_flight = val;
        self
    }

    /// Maximum number of frames to buffer before stopping to read from the
    /// transport
    pub fn max_buffered_frames(&mut self, val: usize) -> &mut Self {
        self.max_buffered_frames = val;
        self
    }

    /// Set the maximum response displacement
    ///
    /// For each request, the response displacement is the number of requests
    /// that arrived after and were completed before. A value of 0 is equivalent
    /// to pipelining for protocols without streaming bodies.
    ///
    /// Default: `usize::MAX`
    pub fn max_response_displacement(&mut self, val: u64) -> &mut Self {
        self.max_response_displacement = val;
        self
    }
}

impl Default for MultiplexConfig {
    fn default() -> Self {
        MultiplexConfig {
            max_in_flight: 100,
            max_buffered_frames: 128,
            max_response_displacement: u64::MAX,
        }
    }
}

/// Additional transport details relevant to streaming, multiplexed protocols.
///
/// All methods added in this trait have default implementations.
pub trait Transport<ReadBody>: 'static +
    Stream<Error = io::Error> +
    Sink<SinkError = io::Error>
{
    /// Allow the transport to do miscellaneous work (e.g., sending ping-pong
    /// messages) that is not directly connected to sending or receiving frames.
    ///
    /// This method should be called every time the task using the transport is
    /// executing.
    fn tick(&mut self) {}

    /// Cancel interest in the exchange identified by RequestId
    fn cancel(&mut self, request_id: RequestId) -> io::Result<()> {
        drop(request_id);
        Ok(())
    }

    /// Tests to see if this I/O object may accept a body frame for the given
    /// request ID
    fn poll_write_body(&mut self, id: RequestId) -> Async<()> {
        drop(id);
        Async::Ready(())
    }

    /// Invoked before the multiplexer dispatches the body chunk to the body
    /// stream.
    fn dispatching_body(&mut self, id: RequestId, body: &ReadBody) {
        drop(id);
        drop(body);
    }
}

impl<T:Io + 'static, C: Codec + 'static, ReadBody> Transport<ReadBody> for Framed<T,C> {}
