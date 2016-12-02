//! Pipelined, multiplexed protocols.
//!
//! See the crate-level docs for an overview.

use std::io;
use futures::{Stream, Sink, Async};

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
