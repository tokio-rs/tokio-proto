//! This library provides a toolkit for rapid protocol development and usage,
//! working with the rest of the Tokio stack.
//!
//! You can find extensive documentation and tutorials in addition to this
//! documentation at [https://tokio.rs](https://tokio.rs)
//!
//! # Protocols
//!
//! Here, a **protocol** is a way of providing or consuming a service. Protocols
//! are implemented via traits, which are arranged into a taxonomy:
//!
//! - `pipeline::{ClientProto, ServerProto}`
//! - `multiplex::{ClientProto, ServerProto}`
//! - `streaming::pipeline::{ClientProto, ServerProto}`
//! - `streaming::multiplex::{ClientProto, ServerProto}`
//!
//! ### Pipeline vs multiplex
//!
//! By default, protocols allow a client to transmit multiple requests without
//! waiting for the corresponding responses, which is commonly used to improve
//! the throughput of single connections.
//!
//! In a **pipelined protocol**, the server responds to client requests in the
//! order they were sent. Example pipelined protocols include HTTP/1.1 and Redis.
//! Pipelining with the max number of in-flight requests set to 1 implies that
//! for each request, the response must be received before sending another
//! request on the same connection.
//!
//! In a **multiplexed protocol**, the server responds to client requests in the
//! order of completion. Request IDs are used to match responses back to requests.
//!
//! In both cases, if multiple requests are sent, the service running on the
//! server *may* process them concurrently, although many services will impose
//! some restrictions depending on the request type.
//!
//! ### Streaming
//!
//! In a non-streaming protocols, the client sends a complete request in a
//! single message, and the server provides a complete response in a single
//! message. Protocol tools in this style are available in the top-level `pipeline`
//! and `multiplex` modules.
//!
//! In a **streaming protocol**, requests and responses can carry **body
//! streams**, which allows partial processing before the complete body has been
//! transferred. Streaming protocol tools are found within the `streaming`
//! submodule.
//!
//! # Transports
//!
//! A key part of any protocol is its **transport**, which is the way that it
//! sends and receives *frames* on its connection. For simple protocols, these
//! frames correspond directly to complete requests and responses. For more
//! complicated protocols, they carry additional metadata, and may only be one
//! part of a request or response body.
//!
//! Transports are defined by implementing the `transport::Transport` trait. The
//! `transport::CodecTransport` type can be used to wrap a `Codec` (from
//! `tokio-core`), which is a simple way to build a transport.
//!
//! # An example server
//!
//! The following code shows how to implement a simple server that receives
//! newline-separated integer values, doubles them, and returns them. It
//! illustrates several aspects of the Tokio stack:
//!
//! - Implementing a codec `IntCodec` for reading and writing integers from a
//!   byte buffer.
//! - Implementing a server protocol `IntProto` using this codec as a transport.
//! - Implementing a service `Doubler` for doubling integers.
//! - Spinning up this service on a local port (in `main`).
//!
//! ```no_run
//! extern crate futures;
//! extern crate tokio_core;
//! extern crate tokio_proto;
//! extern crate tokio_service;
//!
//! use std::str;
//! use std::io::{self, ErrorKind, Write};
//!
//! use futures::{future, Future, BoxFuture};
//! use tokio_core::io::{Io, Codec, Framed, EasyBuf};
//! use tokio_proto::TcpServer;
//! use tokio_proto::pipeline::ServerProto;
//! use tokio_service::Service;
//!
//! // First, we implement a *codec*, which provides a way of encoding and
//! // decoding messages for the protocol. See the documentation for `Codec` in
//! // `tokio-core` for more details on how that works.
//!
//! #[derive(Default)]
//! pub struct IntCodec;
//!
//! fn parse_u64(from: &[u8]) -> Result<u64, io::Error> {
//!     Ok(str::from_utf8(from)
//!        .map_err(|e| io::Error::new(ErrorKind::InvalidData, e))?
//!        .parse()
//!        .map_err(|e| io::Error::new(ErrorKind::InvalidData, e))?)
//! }
//!
//! impl Codec for IntCodec {
//!     type In = u64;
//!     type Out = u64;
//!
//!     // Attempt to decode a message from the given buffer if a complete
//!     // message is available; returns `Ok(None)` if the buffer does not yet
//!     // hold a complete message.
//!     fn decode(&mut self, buf: &mut EasyBuf) -> Result<Option<u64>, io::Error> {
//!         if let Some(i) = buf.as_slice().iter().position(|&b| b == b'\n') {
//!             // remove the line, including the '\n', from the buffer
//!             let full_line = buf.drain_to(i + 1);
//!
//!             // strip the'`\n'
//!             let slice = &full_line.as_slice()[..i];
//!
//!             Ok(Some(parse_u64(slice)?))
//!         } else {
//!             Ok(None)
//!         }
//!     }
//!
//!     // Attempt to decode a message assuming that the given buffer contains
//!     // *all* remaining input data.
//!     fn decode_eof(&mut self, buf: &mut EasyBuf) -> io::Result<u64> {
//!         let amt = buf.len();
//!         Ok(parse_u64(buf.drain_to(amt).as_slice())?)
//!     }
//!
//!     fn encode(&mut self, item: u64, into: &mut Vec<u8>) -> io::Result<()> {
//!         writeln!(into, "{}", item);
//!         Ok(())
//!     }
//! }
//!
//! // Next, we implement the server protocol, which just hooks up the codec above.
//!
//! pub struct IntProto;
//!
//! impl<T: Io + 'static> ServerProto<T> for IntProto {
//!     type Request = u64;
//!     type Response = u64;
//!     type Transport = Framed<T, IntCodec>;
//!     type BindTransport = Result<Self::Transport, io::Error>;
//!
//!     fn bind_transport(&self, io: T) -> Self::BindTransport {
//!         Ok(io.framed(IntCodec))
//!     }
//! }
//!
//! // Now we implement a service we'd like to run on top of this protocol
//!
//! pub struct Doubler;
//!
//! impl Service for Doubler {
//!     type Request = u64;
//!     type Response = u64;
//!     type Error = io::Error;
//!     type Future = BoxFuture<u64, io::Error>;
//!
//!     fn call(&self, req: u64) -> Self::Future {
//!         // Just return the request, doubled
//!         future::finished(req * 2).boxed()
//!     }
//! }
//!
//! // Finally, we can actually host this service locally!
//! fn main() {
//!     let addr = "0.0.0.0:12345".parse().unwrap();
//!     TcpServer::new(IntProto, addr)
//!         .serve(|| Ok(Doubler));
//! }
//! ```

#![doc(html_root_url = "https://docs.rs/tokio-proto/0.1")]
#![deny(warnings, missing_docs, missing_debug_implementations)]
#![allow(deprecated)] // TODO remove this

extern crate net2;
extern crate rand;
extern crate slab;
extern crate smallvec;
extern crate take;
extern crate tokio_core;
extern crate tokio_io;
extern crate tokio_service;

#[macro_use]
extern crate futures;

#[macro_use]
extern crate log;

mod simple;
pub use simple::{pipeline, multiplex};

pub mod streaming;
pub mod util;

mod tcp_client;
pub use tcp_client::{TcpClient, Connect};

mod tcp_server;
pub use tcp_server::TcpServer;

use tokio_core::reactor::Handle;
use tokio_service::Service;

// TODO: move this into futures-rs
mod buffer_one;

/// Binds a service to an I/O object.
///
/// This trait is not intended to be implemented directly; instead, implement
/// one of the server protocol traits:
///
/// - `pipeline::ServerProto`
/// - `multiplex::ServerProto`
/// - `streaming::pipeline::ServerProto`
/// - `streaming::multiplex::ServerProto`
///
/// See the crate documentation for more details on those traits.
///
/// The `Kind` parameter, in particular, is a zero-sized type used to allow
/// blanket implementation from the various protocol traits. Any additional
/// implementations of this trait should use their own zero-sized kind type to
/// distinguish them.
pub trait BindServer<Kind, T: 'static>: 'static {
    /// The request type for the service.
    type ServiceRequest;

    /// The response type for the service.
    type ServiceResponse;

    /// The error type for the service.
    type ServiceError;

    /// Bind the service.
    ///
    /// This method should spawn a new task on the given event loop handle which
    /// provides the given service on the given I/O object.
    fn bind_server<S>(&self, handle: &Handle, io: T, service: S)
        where S: Service<Request = Self::ServiceRequest,
                         Response = Self::ServiceResponse,
                         Error = Self::ServiceError> + 'static;
}

/// Binds an I/O object as a client of a service.
///
/// This trait is not intended to be implemented directly; instead, implement
/// one of the server protocol traits:
///
/// - `pipeline::ClientProto`
/// - `multiplex::ClientProto`
/// - `streaming::pipeline::ClientProto`
/// - `streaming::multiplex::ClientProto`
///
/// See the crate documentation for more details on those traits.
///
/// The `Kind` parameter, in particular, is a zero-sized type used to allow
/// blanket implementation from the various protocol traits. Any additional
/// implementations of this trait should use their own zero-sized kind type to
/// distinguish them.
pub trait BindClient<Kind, T: 'static>: 'static {
    /// The request type for the service.
    type ServiceRequest;

    /// The response type for the service.
    type ServiceResponse;

    /// The error type for the service.
    type ServiceError;

    /// The bound service.
    type BindClient: Service<Request = Self::ServiceRequest,
                             Response = Self::ServiceResponse,
                             Error = Self::ServiceError>;

    /// Bind an I/O object as a service.
    fn bind_client(&self, handle: &Handle, io: T) -> Self::BindClient;
}
