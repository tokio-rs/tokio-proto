use std::collections::VecDeque;
use std::io;

use futures::stream::Stream;
use futures::{self, Future, BoxFuture, Complete, Async};
use tokio_core::{Sender, Receiver, LoopHandle};

use Service;
use super::{pipeline, Error, Message, Transport, NewTransport};

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

/// Connect to the given `addr` and handle using the given Transport and protocol pipelining.
pub fn connect<T, B, E>(handle: LoopHandle, new_transport: T)
        -> Client<T::In, T::Out, B, E>
    where T: NewTransport<Error = E> + Send + 'static,
          T::In: Send + 'static,
          T::Out: Send + 'static,
          B: Stream<Item = T::BodyIn, Error = E> + Send + 'static,
          E: From<Error<E>> + Send + 'static,
{
    let (tx, rx) = handle.clone().channel();

    handle.spawn(|_| {
        rx.and_then(move |rx| {
            // Create the transport
            let transport = try!(new_transport.new_transport());

            // Create the client dispatch
            let dispatch: Dispatch<T::Item, B, E> = Dispatch {
                requests: rx,
                in_flight: VecDeque::with_capacity(32),
            };

            // Create the pipeline with the dispatch and transport
            let pipeline = try!(pipeline::Pipeline::new(dispatch, transport));
            Ok(pipeline)
        }).flatten().map_err(|e| {
            // TODO: where to punt this error to?
            error!("pipeline error: {}", e)
        })
    });

    Client { tx: tx }
}

impl<Req, Resp, ReqBody, E> Service for Client<Req, Resp, ReqBody, E>
    where Req: Send + 'static,
          Resp: Send + 'static,
          ReqBody: Stream<Error = E>,
          E: From<Error<E>> + Send + 'static,
{
    type Req = Message<Req, ReqBody>;
    type Resp = Resp;
    type Error = E;
    type Fut = BoxFuture<Self::Resp, E>;

    fn call(&self, request: Self::Req) -> Self::Fut {
        let (tx, rx) = futures::oneshot();

        // TODO: handle error
        self.tx.send((request, tx)).ok().unwrap();

        rx.then(|t| t.unwrap()).boxed()
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
