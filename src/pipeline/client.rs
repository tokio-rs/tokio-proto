use {Error, Body, Message};
use super::{Transport};
use super::pipeline::{self, Pipeline, PipelineMessage};
use client::{self, Client, Receiver};
use futures::stream::Stream;
use futures::{Future, IntoFuture, Complete, Poll, Async};
use tokio_core::reactor::Handle;
use std::collections::VecDeque;
use std::io;

struct Dispatch<T, B>
    where T: Transport,
          B: Stream<Item = T::BodyIn, Error = T::Error>,
{
    transport: T,
    requests: Receiver<T::In, T::Out, B, Body<T::BodyOut, T::Error>, T::Error>,
    in_flight: VecDeque<Complete<Result<Message<T::Out, Body<T::BodyOut, T::Error>>, T::Error>>>,
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
                in_flight: VecDeque::with_capacity(32),
            };

            Pipeline::new(dispatch)
        })
        .map_err(|e| {
            // TODO: where to punt this error to?
            error!("pipeline error: {}", e);
        });

    // Spawn the task
    handle.spawn(task);

    // Return the client
    client
}

impl<T, B> pipeline::Dispatch for Dispatch<T, B>
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

    fn dispatch(&mut self, response: PipelineMessage<Self::Out, Body<Self::BodyOut, Self::Error>, Self::Error>) -> io::Result<()> {
        if let Some(complete) = self.in_flight.pop_front() {
            complete.complete(response);
        } else {
            return Err(io::Error::new(io::ErrorKind::Other, "request / response mismatch"));
        }

        Ok(())
    }

    fn poll(&mut self) -> Poll<Option<PipelineMessage<Self::In, Self::Stream, Self::Error>>, io::Error> {
        trace!("Dispatch::poll");
        // Try to get a new request frame
        match self.requests.poll() {
            Ok(Async::Ready(Some((request, complete)))) => {
                trace!("   --> received request");

                // Track complete handle
                self.in_flight.push_back(complete);

                Ok(Async::Ready(Some(Ok(request))))

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

    fn has_in_flight(&self) -> bool {
        !self.in_flight.is_empty()
    }
}

impl<T, B> Drop for Dispatch<T, B>
    where T: Transport,
          B: Stream<Item = T::BodyIn, Error = T::Error>,
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
