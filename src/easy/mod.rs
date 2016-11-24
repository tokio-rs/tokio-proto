//! "Easy" and simplified interfaces to tokio-proto APIs.
//!
//! The tokio-proto `pipeline` and `multiplex` modules are relatively complex,
//! namely because of their ability to handle streaming bodies in both requests
//! and responses. Not all protocols, however, need this functionality. To
//! easily get off the ground running with these kinds of protocols, this module
//! contains simplified variants of the pipeline/multiple server/client halves
//! to forgo streaming bodies and instead just deal with requests and responses.
//!
//! Clients and servers created from the easy module only require an instance of
//! `FramedIo`, a trait defined in the `tokio-core` crate for decoding and
//! encoding frames from a byte stream. The frames defined by `FramedIo` are
//! then the request/response types for the clients and servers defined here.
//!
//! Note that this module is intended to be paired with `tokio_core::easy` as
//! well where an instance of `EasyFramedIo` will slot nicely into all of the
//! implementations here as well.
//!
//! Internally it's also important to note that all support in this module is
//! built on top of the `pipeline` and `multiplex` modules in this crate as
//! well, with most types just taken care of for you.

use std::io;

use futures::stream::Receiver;
use futures::{Future, Poll};
use tokio_service::Service;

use {Message, Body};
use client::{Response, Client};

/// A client which does not support streaming bodies.
///
/// This client is returned from the `easy::pipeline::connect` and
/// `easy::multiplex::connect` functions in this crate. The client's requests
/// and responses do not support streaming bodies. Each client implements the
/// `Service` trait to express sending a request to the remote end of this
/// client, and the future returned represents the response. Responses are
/// automatically managed according to the internal protocol.
///
/// The `R1` type parameter here is the type of requests that this client can
/// send, and is also the argument for the `Service::call` method trait
/// implementation.
///
/// The `R2` type parameter is the type of response that will be received and
/// is the resolved value of the future returned by `call`.
///
/// Note that currently this client is not generic over errors as well, but
/// rather all errors returned are instances of `io::Error`.
pub struct EasyClient<R1, R2> {
    inner: Client<R1,
                  R2,
                  Receiver<(), io::Error>,
                  Body<(), io::Error>,
                  io::Error>,
}

impl<R1, R2> Service for EasyClient<R1, R2>
    where R1: 'static,
          R2: 'static,
{
    type Request = R1;
    type Response = R2;
    type Error = io::Error;
    type Future = EasyResponse<R2>;

    fn call(&self, request: R1) -> EasyResponse<R2> {
        EasyResponse {
            inner: self.inner.call(Message::WithoutBody(request)),
        }
    }
}

/// Future returned from `Client::call`.
///
/// This future returned from `Client::call` represents the response to a
/// particular request. This future will either resolve to `T` or and
/// `io::Error`.
pub struct EasyResponse<T> {
    inner: Response<Message<T, Body<(), io::Error>>,
                    io::Error>,
}

impl<T> Future for EasyResponse<T> {
    type Item = T;
    type Error = io::Error;

    fn poll(&mut self) -> Poll<T, io::Error> {
        match try_ready!(self.inner.poll()) {
            Message::WithoutBody(t) => Ok(t.into()),
            Message::WithBody(..) => panic!("easy has no body"),
        }
    }
}

pub mod multiplex;
pub mod pipeline;

