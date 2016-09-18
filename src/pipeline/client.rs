use std::collections::VecDeque;
use std::io;
use std::marker::PhantomData;

use futures::stream::Stream;
use futures::{self, Future, BoxFuture, Complete, Async, Poll};
use tokio_core::reactor::Handle;
use tokio_core::channel::{channel, Sender, Receiver};

use tokio_service::Service;
use {Error};
use super::{pipeline, Message, Transport, NewTransport};

/// Client `Service` for the pipeline protocol.
pub struct Client<Req, Resp, ReqBody, E>
    where ReqBody: Stream<Error = E>,
          E: From<Error<E>>,
{
    tx: Sender<(Message<Req, ReqBody>, Complete<Result<Resp, E>>)>,
}

struct Dispatch<T, B, E>
    where T: Transport<Error = E>,
          B: Stream<Item = T::BodyIn, Error = E>,
          E: From<Error<E>>,
{
    requests: Receiver<(Message<T::In, B>, Complete<Result<T::Out, E>>)>,
    in_flight: VecDeque<Complete<Result<T::Out, E>>>,
}

/// Connecting
pub struct Connecting<T, C> {
    reactor: Handle,
    transport: T,
    client: PhantomData<C>,
}

impl<F, B, E> Future for Connecting<F, Client<<F::Item as Transport>::In, <F::Item as Transport>::Out, B, E>>
    where F: Future<Error = io::Error>,
          F::Item: Transport<Error = E> + Send + 'static,
          <F::Item as Transport>::In: Send + 'static,
          <F::Item as Transport>::Out: Send + 'static,
          B: Stream<Item = <F::Item as Transport>::BodyIn, Error = E> + Send + 'static,
          E: From<Error<E>> + Send + 'static,
{
    type Item = Client<<F::Item as Transport>::In, <F::Item as Transport>::Out, B, E>;
    type Error = io::Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let transport = try_ready!(self.transport.poll());

        // Channel over which the client handle sends requests
        let (tx, rx) = try!(channel(&self.reactor));

        // Create the client dispatch
        let dispatch: Dispatch<F::Item, B, E> = Dispatch {
            requests: rx,
            in_flight: VecDeque::with_capacity(32),
        };

        // Create the pipeline with the dispatch and transport
        let pipeline = try!(pipeline::Pipeline::new(dispatch, transport));

        self.reactor.spawn(pipeline.map_err(|e| {
            // TODO: where to punt this error to?
            error!("pipeline error: {}", e)
        }));

        Ok(Async::Ready(Client { tx: tx }))
    }
}

/// Connect to the given `addr` and handle using the given Transport and protocol pipelining.
pub fn connect<T, B, E>(new_transport: T, handle: &Handle) -> Connecting<T::Future, Client<T::In, T::Out, B, E>>
    where T: NewTransport<Error = E>,
          T::In: Send + 'static,
          T::Out: Send + 'static,
          T::Item: Send + 'static,
          B: Stream<Item = T::BodyIn, Error = E> + Send + 'static,
          E: From<Error<E>> + Send + 'static,
{
    Connecting {
        reactor: handle.clone(),
        transport: new_transport.new_transport(),
        client: PhantomData,
    }
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
        self.tx.send((request, tx)).ok().unwrap();

        rx.then(|t| t.unwrap()).boxed()
    }

    fn poll_ready(&self) -> Async<()> {
        Async::Ready(())
    }
}

impl<Req, Resp, ReqBody, E> Clone for Client<Req, Resp, ReqBody, E>
    where ReqBody: Stream<Error = E>,
          E: From<Error<E>>,
{
    fn clone(&self) -> Client<Req, Resp, ReqBody, E> {
        Client { tx: self.tx.clone() }
    }
}

impl<T, B, E> pipeline::Dispatch for Dispatch<T, B, E>
    where T: Transport<Error = E>,
          B: Stream<Item = T::BodyIn, Error = E>,
          E: From<Error<E>>,
{
    type InMsg = T::In;
    type InBody = T::BodyIn;
    type InBodyStream = B;
    type OutMsg = T::Out;
    type Error = E;

    fn dispatch(&mut self, response: Self::OutMsg) -> io::Result<()> {
        if let Some(complete) = self.in_flight.pop_front() {
            complete.complete(Ok(response));
        } else {
            return Err(io::Error::new(io::ErrorKind::Other, "request / response mismatch"));
        }

        Ok(())
    }

    fn poll(&mut self) -> Option<Result<Message<Self::InMsg, Self::InBodyStream>, Self::Error>> {
        // Try to get a new request frame
        match self.requests.poll() {
            Ok(Async::Ready(Some((request, complete)))) => {
                trace!("received request");

                // Track complete handle
                self.in_flight.push_back(complete);

                Some(Ok(request))

            }
            Ok(Async::Ready(None)) => None,
            Err(e) => {
                // An error on receive can only happen when the other half
                // disconnected. In this case, the client needs to be
                // shutdown
                panic!("unimplemented error handling: {:?}", e);
            }
            Ok(Async::NotReady) => None,
        }
    }

    fn has_in_flight(&self) -> bool {
        !self.in_flight.is_empty()
    }
}

impl<T, B, E> Drop for Dispatch<T, B, E>
    where T: Transport<Error = E>,
          B: Stream<Item = T::BodyIn, Error = E>,
          E: From<Error<E>>,
{
    fn drop(&mut self) {
        // Complete any pending requests with an error
        while let Some(complete) = self.in_flight.pop_front() {
            let err = Error::Io(broken_pipe());
            complete.complete(Err(err.into()));
        }
    }
}

fn broken_pipe() -> io::Error {
    io::Error::new(io::ErrorKind::BrokenPipe, "broken pipe")
}
