//! Utilities for building protocol clients
//!
//! Provides a channel that handles details of providing a `Service` client.
//! Usually, this module does not have to be used directly. Instead it is used
//! by `pipeline` and `multiplex` in the `connect` fns.
//!
//! However, some protocols require implementing the dispatch layer directly,
//! in which case using client channel is helpful.

// Allow warnings in order to prevent the compiler from outputting an error
// that seems to be fixed on nightly.
#![allow(warnings)]

use {Error, Message};
use sender::Sender;
use tokio_service::Service;
use futures::stream::{self, Stream};
use futures::{self, Future, Oneshot, Complete, Async, Poll};
use std::io;
use std::cell::RefCell;

/// Client `Service` for pipeline or multiplex protocols
pub struct Client<R1, R2, B1, B2, E>
    where B1: Stream<Error = E>,
          E: From<Error<E>>,
{
    tx: RefCell<Sender<Envelope<R1, R2, B1, B2, E>, io::Error>>,
}

/// Response future returned from a client
pub struct Response<T, E> {
    inner: Oneshot<Result<T, E>>,
}

/// Message used to dispatch requests to the task managing the client
/// connection.
type Envelope<R1, R2, B1, B2, E> =
    (Message<R1, B1>, Complete<Result<Message<R2, B2>, E>>);

/// A client / receiver pair
pub type Pair<R1, R2, B1, B2, E> =
    (Client<R1, R2, B1, B2, E>, Receiver<R1, R2, B1, B2, E>);

/// Receive requests submitted to the client
pub type Receiver<R1, R2, B1, B2, E> =
    stream::Receiver<Envelope<R1, R2, B1, B2, E>, io::Error>;

/// Return a client handle and a handle used to receive requests on
pub fn pair<R1, R2, B1, B2, E>() -> Pair<R1, R2, B1, B2, E>
    where B1: Stream<Error = E>,
          E: From<Error<E>>,
{
    // Create a stream
    let (tx, rx) = stream::channel();

    // Use the sender handle to create a `Client` handle
    let client = Client { tx: RefCell::new(tx.into()) };

    // Return the pair
    (client, rx)
}

impl<R1, R2, B1, B2, E> Service for Client<R1, R2, B1, B2, E>
    where R1: 'static,
          R2: 'static,
          B1: Stream<Error = E> + 'static,
          B2: 'static,
          E: From<Error<E>> + 'static,
{
    type Request = Message<R1, B1>;
    type Response = Message<R2, B2>;
    type Error = E;
    type Future = Response<Self::Response, E>;

    fn call(&self, request: Self::Request) -> Self::Future {
        let (tx, rx) = futures::oneshot();

        // TODO: handle error
        self.tx.borrow_mut().send(Ok((request, tx)));

        Response { inner: rx }
    }
}

impl<T, E> Future for Response<T, E>
    where E: From<Error<E>>,
{
    type Item = T;
    type Error = E;

    fn poll(&mut self) -> Poll<T, E> {
        match self.inner.poll() {
            Ok(Async::Ready(Ok(v))) => Ok(Async::Ready(v)),
            Ok(Async::Ready(Err(e))) => Err(e),
            Ok(Async::NotReady) => Ok(Async::NotReady),
            Err(_) => {
                let e = Error::Io(io::Error::new(io::ErrorKind::BrokenPipe, "broken pipe"));
                Err(e.into())
            }
        }
    }
}
