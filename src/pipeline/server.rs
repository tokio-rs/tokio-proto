use {Message, Body};
use super::{pipeline, Transport, Pipeline, PipelineMessage};
use tokio_service::Service;
use futures::{Future, Poll, Async};
use futures::stream::Stream;
use std::collections::VecDeque;
use std::io;

// TODO:
//
// - Wait for service readiness
// - Handle request body stream cancellation

/// A server `Task` that dispatches `Transport` messages to a `Service` using
/// protocol pipelining.
pub struct Server<S, T, B>
    where T: Transport,
          S: Service<Request = Message<T::Out, Body<T::BodyOut, T::Error>>,
                    Response = Message<T::In, B>,
                       Error = T::Error> + 'static,
          B: Stream<Item = T::BodyIn, Error = T::Error> + 'static,
{
    inner: Pipeline<Dispatch<S, T>>,
}

struct Dispatch<S, T> where S: Service {
    // The service handling the connection
    service: S,
    transport: T,
    in_flight: VecDeque<InFlight<S::Future>>,
}

enum InFlight<F: Future> {
    Active(F),
    Done(Result<F::Item, F::Error>),
}

impl<S, T, B> Server<S, T, B>
    where T: Transport,
          S: Service<Request = Message<T::Out, Body<T::BodyOut, T::Error>>,
                    Response = Message<T::In, B>,
                       Error = T::Error> + 'static,
          B: Stream<Item = T::BodyIn, Error = T::Error>,
{
    /// Create a new pipeline `Server` dispatcher with the given service and
    /// transport
    pub fn new(service: S, transport: T) -> Server<S, T, B> {
        let dispatch = Dispatch {
            service: service,
            transport: transport,
            in_flight: VecDeque::with_capacity(32),
        };

        // Create the pipeline dispatcher
        let pipeline = Pipeline::new(dispatch);

        // Return the server task
        Server { inner: pipeline }
    }
}

impl<S, T, B> pipeline::Dispatch for Dispatch<S, T>
    where T: Transport,
          S: Service<Request = Message<T::Out, Body<T::BodyOut, T::Error>>,
                    Response = Message<T::In, B>,
                       Error = T::Error> + 'static,
          B: Stream<Item = T::BodyIn, Error = T::Error> + 'static,
{
    type In = T::In;
    type BodyIn = T::BodyIn;
    type Out = T::Out;
    type BodyOut = T::BodyOut;
    type Error = T::Error;
    type Stream = B;
    type Transport = T;

    fn transport(&mut self) -> &mut T {
        &mut self.transport
    }

    fn dispatch(&mut self,
                request: PipelineMessage<Self::Out, Body<Self::BodyOut, Self::Error>, Self::Error>)
                -> io::Result<()>
    {
        if let Ok(request) = request {
            let response = self.service.call(request);
            self.in_flight.push_back(InFlight::Active(response));
        }

        // TODO: Should the error be handled differently?

        Ok(())
    }

    fn poll(&mut self) -> Poll<Option<PipelineMessage<Self::In, Self::Stream, Self::Error>>, io::Error> {
        for slot in self.in_flight.iter_mut() {
            slot.poll();
        }

        match self.in_flight.front() {
            Some(&InFlight::Done(_)) => {}
            _ => return Ok(Async::NotReady)
        }

        match self.in_flight.pop_front() {
            Some(InFlight::Done(res)) => Ok(Async::Ready(Some(res))),
            _ => panic!(),
        }
    }

    fn has_in_flight(&self) -> bool {
        !self.in_flight.is_empty()
    }
}

impl<F: Future> InFlight<F> {
    fn poll(&mut self) {
        let res = match *self {
            InFlight::Active(ref mut f) => {
                match f.poll() {
                    Ok(Async::Ready(e)) => Ok(e),
                    Err(e) => Err(e),
                    Ok(Async::NotReady) => return,
                }
            }
            _ => return,
        };
        *self = InFlight::Done(res);
    }
}

impl<S, T, B> Future for Server<S, T, B>
    where T: Transport,
          S: Service<Request = Message<T::Out, Body<T::BodyOut, T::Error>>,
                    Response = Message<T::In, B>,
                       Error = T::Error> + 'static,
          B: Stream<Item = T::BodyIn, Error = T::Error>,
{
    type Item = ();
    type Error = io::Error;

    fn poll(&mut self) -> Poll<(), io::Error> {
        self.inner.poll()
    }
}
