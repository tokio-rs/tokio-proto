//! Pipelined, streaming protocols.
//!
//! See the crate-level docs for an overview.

use std::io;
use futures::{Stream, Sink};
use tokio_core::io as old_io;
use tokio_io as new_io;

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
#[derive(Debug)]
pub struct StreamingPipeline<B>(B);

/// Additional transport details relevant to streaming, pipelined protocols.
///
/// All methods added in this trait have default implementations.
pub trait Transport: 'static +
    Stream<Error = io::Error> +
    Sink<SinkError = io::Error>
{
    /// Allow the transport to do miscellaneous work (e.g., sending ping-pong
    /// messages) that is not directly connected to sending or receiving frames.
    ///
    /// This method should be called every time the task using the transport is
    /// executing.
    fn tick(&mut self) {}

    /// Cancel interest in the current stream
    fn cancel(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl<T, C> Transport for old_io::Framed<T,C>
    where T: old_io::Io + 'static,
          C: old_io::Codec + 'static,
{}

impl<T, C> Transport for new_io::codec::Framed<T,C>
    where T: new_io::AsyncRead + new_io::AsyncWrite + 'static,
          C: new_io::codec::Encoder<Error=io::Error> +
                new_io::codec::Decoder<Error=io::Error> + 'static,
{}
