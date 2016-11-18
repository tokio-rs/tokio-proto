//! Pipelined, streaming protocols.
//!
//! See the crate-level docs for an overview.

use std::io;
use transport::Transport;

mod frame;
pub use self::frame::Frame;

mod client;
pub use self::client::ClientProto;

mod server;
pub use self::server::ServerProto;

pub mod advanced;

/// A marker used to flag protocols as being streaming and pipelined.
///
/// This is an implementation detail; to actually implement a protocol,
/// implement the `ClientProto` or `ServerProto` traits in this module.
pub struct StreamingPipeline<B>(B);

/// Additional transport details relevant to streaming, pipelined protocols.
///
/// All methods added in this trait have default implementations.
pub trait StreamingTransport<T>: Transport<T> {
    /// Cancel interest in the current stream
    fn cancel(&mut self) -> io::Result<()> {
        Ok(())
    }
}
