//! Utilities for building protocol clients
//!
//! Provides a channel that handles details of providing a `Service` client.
//! Usually, this module does not have to be used directly. Instead it is used
//! by `pipeline` and `multiplex` in the `connect` fns.
//!
//! However, some protocols require implementing the dispatch layer directly,
//! in which case using client channel is helpful.

use {Error, Message};
use sender::Sender;
use tokio_service::Service;
use futures::stream::{self, Stream};
use futures::{self, Future, BoxFuture, Complete, Async};
use std::io;
use std::cell::RefCell;

/// Client `Service` for pipeline or multiplex protocols
pub struct Client<Req, Resp, ReqBody, E>
    where ReqBody: Stream<Error = E>,
          E: From<Error<E>>,
{
    tx: RefCell<Sender<Envelope<Req, Resp, ReqBody, E>, io::Error>>,
}

/// Message used to dispatch requests to the task managing the client
/// connection.
type Envelope<Req, Resp, ReqBody, E> =
    (Message<Req, ReqBody>, Complete<Result<Resp, E>>);

/// A client / receiver pair
type Pair<Req, Resp, ReqBody, E> =
    (Client<Req, Resp, ReqBody, E>, Receiver<Req, Resp, ReqBody, E>);

/// Receive requests submitted to the client
pub type Receiver<Req, Resp, ReqBody, E> =
    stream::Receiver<Envelope<Req, Resp, ReqBody, E>, io::Error>;

/// Return a client handle and a handle used to receive requests on
pub fn pair<Req, Resp, ReqBody, E>() -> Pair<Req, Resp, ReqBody, E>
    where ReqBody: Stream<Error = E>,
          E: From<Error<E>>,
{
    // Create a stream
    let (tx, rx) = stream::channel();

    // Use the sender handle to create a `Client` handle
    let client = Client { tx: RefCell::new(tx.into()) };

    // Return the pair
    (client, rx)
}

impl<Req, Resp, ReqBody, E> Service for Client<Req, Resp, ReqBody, E>
    where Req: Send + 'static,
          Resp: Send + 'static,
          ReqBody: Stream<Error = E>,
          E: From<Error<E>> + Send + 'static,
{
    type Request = Message<Req, ReqBody>;
    type Response = Resp;
    type Error = E;
    type Future = BoxFuture<Self::Response, E>;

    fn call(&self, request: Self::Request) -> Self::Future {
        let (tx, rx) = futures::oneshot();

        // TODO: handle error
        self.tx.borrow_mut().send(Ok((request, tx)));

        rx.then(|t| t.unwrap()).boxed()
    }

    fn poll_ready(&self) -> Async<()> {
        self.tx.borrow_mut().poll_ready().unwrap()
    }
}
