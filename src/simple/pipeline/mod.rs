//! Pipelined RPC protocols.
//!
//! See the crate-level docs for an overview.

use streaming::pipeline::Frame;

mod client;
pub use self::client::ClientProto;
pub use self::client::ClientService;

mod server;
pub use self::server::ServerProto;

/// A marker used to flag protocols as being pipelined RPC.
///
/// This is an implementation detail; to actually implement a protocol,
/// implement the `ClientProto` or `ServerProto` traits in this module.
pub struct Pipeline;

fn lift_msg_result<T, E>(msg: Result<T, E>) -> Frame<T, (), E> {
    match msg {
        Ok(msg) => Frame::Message { message: msg, body: false },
        Err(err) => Frame::Error { error: err },
    }
}

fn lower_msg_result<T, E>(msg: Frame<T, (), E>) -> Result<T, E> {
    match msg {
        Frame::Message { message, body } => {
            if !body { return Ok(message) }
        }
        Frame::Error { error } => return Err(error),
        _ => {}
    }
    panic!("Encountered a streaming body in a non-streaming protocol")
}

fn lift_msg<T, E>(msg: T) -> Frame<T, (), E> {
    Frame::Message { message: msg, body: false }
}

fn lower_msg<T, E>(msg: Frame<T, (), E>) -> T {
    match msg {
        Frame::Message { message, body } => {
            if !body { return message }
        }
        Frame::Error { .. } =>
            panic!("Encountered unexpected error frame in a non-streaming protocol"),
        _ => {}
    }
    panic!("Encountered a streaming body in a non-streaming protocol");
}
