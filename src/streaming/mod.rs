//! Streaming protocols.
//!
//! Streaming protocols exchange request and response *headers*, which may come
//! with *streaming bodies*, which are represented as `Stream`s. The protocols
//! come in two forms: pipelined and multiplexed. See the crate-level docs for
//! an overview.

pub mod pipeline;
pub mod multiplex;

mod body;
pub use self::body::Body;

mod message;
pub use self::message::Message;
