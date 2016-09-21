use {Error, Message};
use super::{Transport, NewTransport};
use super::pipeline::{self, Pipeline};
use client::{self, Client, Receiver};
use futures::stream::Stream;
use futures::{Future, Complete, Async};
use tokio_core::reactor::Handle;
use std::collections::VecDeque;
use std::io;

struct Dispatch<T, B, E>
    where T: Transport<Error = E>,
          B: Stream<Item = T::BodyIn, Error = E>,
          E: From<Error<E>>,
{
    requests: Receiver<T::In, T::Out, B, E>,
    in_flight: VecDeque<Complete<Result<T::Out, E>>>,
}

/// Connect to the given `addr` and handle using the given Transport and protocol pipelining.
pub fn connect<T, B, E>(new_transport: T, handle: &Handle) -> Client<T::In, T::Out, B, E>
    where T: NewTransport<Error = E>,
          T::In: Send + 'static,
          T::Out: Send + 'static,
          T::Item: Send + 'static,
          T::Future: Send + 'static,
          B: Stream<Item = T::BodyIn, Error = E> + Send + 'static,
          E: From<Error<E>> + Send + 'static,
{
    let (client, rx) = client::pair();

    // Create the client dispatch
    let dispatch: Dispatch<T::Item, B, E> = Dispatch {
        requests: rx,
        in_flight: VecDeque::with_capacity(32),
    };

    let task = new_transport.new_transport()
        .and_then(|transport| Pipeline::new(dispatch, transport))
        .map_err(|e| {
            // TODO: where to punt this error to?
            error!("pipeline error: {}", e);
        });

    // Spawn the task
    handle.spawn(task);

    // Return the client
    client
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
        trace!("Dispatch::poll");
        // Try to get a new request frame
        match self.requests.poll() {
            Ok(Async::Ready(Some((request, complete)))) => {
                trace!("   --> received request");

                // Track complete handle
                self.in_flight.push_back(complete);

                Some(Ok(request))

            }
            Ok(Async::Ready(None)) => {
                trace!("   --> client dropped");
                None
            }
            Err(e) => {
                trace!("   --> error");
                // An error on receive can only happen when the other half
                // disconnected. In this case, the client needs to be
                // shutdown
                panic!("unimplemented error handling: {:?}", e);
            }
            Ok(Async::NotReady) => {
                trace!("   --> not ready");
                None
            }
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
