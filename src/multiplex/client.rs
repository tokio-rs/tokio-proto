use {Error, Body, Message};
use super::{multiplex, Transport, RequestId, Multiplex, MultiplexMessage};
use client::{self, Client, Receiver};
use futures::{Future, IntoFuture, Complete, Poll, Async};
use futures::stream::Stream;
use tokio_core::reactor::Handle;
use std::io;
use std::collections::HashMap;

struct Dispatch<T, B>
    where T: Transport,
          B: Stream<Item = T::BodyIn, Error = T::Error>,
{
    transport: T,
    requests: Receiver<T::In, T::Out, B, Body<T::BodyOut, T::Error>, T::Error>,
    in_flight: HashMap<RequestId, Complete<Result<Message<T::Out, Body<T::BodyOut, T::Error>>, T::Error>>>,
    next_request_id: u64,
}

/// Connect to the given `addr` and handle using the given Transport and protocol pipelining.
pub fn connect<T, F, B>(new_transport: F, handle: &Handle)
    -> Client<T::In, T::Out, B, Body<T::BodyOut, T::Error>, T::Error>
    where F: IntoFuture<Item = T, Error = io::Error> + 'static,
          T: Transport,
          B: Stream<Item = T::BodyIn, Error = T::Error> + 'static,
{
    let (client, rx) = client::pair();

    let task = new_transport.into_future()
        .and_then(move |transport| {
            let dispatch: Dispatch<T, B> = Dispatch {
                transport: transport,
                requests: rx,
                in_flight: HashMap::new(),
                next_request_id: 0,
            };

            Multiplex::new(dispatch)
        })
        .map_err(|e| {
            // TODO: where to punt this error to?
            error!("multiplex error: {}", e);
        });

    // Spawn the task
    handle.spawn(task);

    // Return the client
    client
}

impl<T, B> multiplex::Dispatch for Dispatch<T, B>
    where T: Transport,
          B: Stream<Item = T::BodyIn, Error = T::Error> + 'static,
{
    type In = T::In;
    type BodyIn = T::BodyIn;
    type Out = T::Out;
    type BodyOut = T::BodyOut;
    type Error = T::Error;
    type Stream = B;
    type Transport = T;

    fn transport(&mut self) -> &mut Self::Transport {
        &mut self.transport
    }

    fn dispatch(&mut self, (id, response): MultiplexMessage<Self::Out, Body<Self::BodyOut, Self::Error>, Self::Error>) -> io::Result<()> {
        if let Some(complete) = self.in_flight.remove(&id) {
            complete.complete(response);
        } else {
            return Err(io::Error::new(io::ErrorKind::Other, "request / response mismatch"));
        }

        Ok(())
    }

    fn poll(&mut self) -> Poll<Option<MultiplexMessage<Self::In, B, Self::Error>>, io::Error> {
        trace!("Dispatch::poll");
        // Try to get a new request frame
        match self.requests.poll() {
            Ok(Async::Ready(Some((request, complete)))) => {
                trace!("   --> received request");

                let request_id = self.next_request_id;
                self.next_request_id += 1;

                trace!("   --> assigning request-id={:?}", request_id);

                // Track complete handle
                self.in_flight.insert(request_id, complete);

                Ok(Async::Ready(Some((request_id, Ok(request)))))

            }
            Ok(Async::Ready(None)) => {
                trace!("   --> client dropped");
                Ok(Async::Ready(None))
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
                Ok(Async::NotReady)
            }
        }
    }

    fn poll_ready(&self) -> Async<()> {
        // Not capping the client yet
        Async::Ready(())
    }

    fn cancel(&mut self, _request_id: RequestId) -> io::Result<()> {
        // TODO: implement
        Ok(())
    }
}

impl<T, B> Drop for Dispatch<T, B>
    where T: Transport,
          B: Stream<Item = T::BodyIn, Error = T::Error>,
{
    fn drop(&mut self) {
        if !self.in_flight.is_empty() {
            warn!("multiplex client dropping with in-flight exchanges");
        }

        // Complete any pending requests with an error
        for (_, complete) in self.in_flight.drain() {
            let err = Error::Io(broken_pipe());
            complete.complete(Err(err.into()));
        }
    }
}

fn broken_pipe() -> io::Error {
    io::Error::new(io::ErrorKind::BrokenPipe, "broken pipe")
}
