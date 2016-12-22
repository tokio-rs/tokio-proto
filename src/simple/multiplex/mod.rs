//! Multiplexed RPC protocols.
//!
//! See the crate-level docs for an overview.

use streaming::multiplex::Frame;

mod client;
pub use self::client::ClientProto;
pub use self::client::ClientService;

mod server;
pub use self::server::ServerProto;

/// Identifies a request / response thread
pub type RequestId = u64;

/// A marker used to flag protocols as being multiplexed RPC.
///
/// This is an implementation detail; to actually implement a protocol,
/// implement the `ClientProto` or `ServerProto` traits in this module.
pub struct Multiplex;

fn lift_msg_result<T, E>(msg: (RequestId, Result<T, E>)) -> Frame<T, (), E> {
    let (id, msg) = msg;
    match msg {
        Ok(msg) => Frame::Message { message: msg, body: false, solo: false, id: id },
        Err(err) => Frame::Error { error: err, id: id },
    }
}

fn lower_msg_result<T, E>(msg: Frame<T, (), E>) -> (RequestId, Result<T, E>) {
    match msg {
        Frame::Message { message, body, solo, id } => {
            if !body && !solo { return (id, Ok(message)) }
        }
        Frame::Error { error, id } => return (id, Err(error)),
        _ => {}
    }
    panic!("Encountered a streaming body in a non-streaming protocol")
}

fn lift_msg<T, E>(msg: (RequestId, T)) -> Frame<T, (), E> {
    Frame::Message { message: msg.1, body: false, solo: false, id: msg.0 }
}

fn lower_msg<T, E>(msg: Frame<T, (), E>) -> (RequestId, T) {
    match msg {
        Frame::Message { message, body, solo, id } => {
            if !body && !solo { return (id, message) }
        }
        Frame::Error { .. } =>
            panic!("Encountered unexpected error frame in a non-streaming protocol"),
        _ => {}
    }
    panic!("Encountered a streaming body in a non-streaming protocol");
}
