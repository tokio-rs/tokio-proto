use super::{pipeline, Error, ServerService, Transport};
use reactor::{Task, Tick};
use util::future::AwaitQueue;
use std::io;

// TODO:
//
// - Wait for service readiness
// - Handle request body stream cancellation

/// A server `Task` that dispatches `Transport` messages to a `Service` using
/// protocol pipelining.
pub struct Server<S, T>
    where S: ServerService,
          T: Transport,
{
    inner: pipeline::Pipeline<Dispatch<S>, T>,
}

struct Dispatch<S: ServerService> {
    // The service handling the connection
    service: S,
    // Don't look at this, it is terrible
    in_flight: AwaitQueue<S::Fut>,
}

impl<T, S, E> Server<S, T>
    where T: Transport<Error = E>,
          S: ServerService<Req = T::Out, Resp = T::In, Body = T::BodyIn, Error = E>,
          E: From<Error<E>> + Send + 'static,
{
    /// Create a new pipeline `Server` dispatcher with the given service and
    /// transport
    pub fn new(service: S, transport: T) -> io::Result<Server<S, T>> {
        let dispatch = Dispatch {
            service: service,
            in_flight: try!(AwaitQueue::with_capacity(32)),
        };

        // Create the pipeline dispatcher
        let pipeline = try!(pipeline::Pipeline::new(dispatch, transport));

        // Return the server task
        Ok(Server { inner: pipeline })
    }
}

impl<S> pipeline::Dispatch for Dispatch<S>
    where S: ServerService,
{
    type InMsg = S::Resp;
    type InBody = S::Body;
    type InBodyStream = S::BodyStream;
    type OutMsg = S::Req;
    type Error = S::Error;

    fn dispatch(&mut self, request: Self::OutMsg) -> io::Result<()> {
        let response = self.service.call(request);
        self.in_flight.push(response);
        Ok(())
    }

    fn poll(&mut self) -> Option<Result<(Self::InMsg, Option<Self::InBodyStream>), Self::Error>> {
        self.in_flight.poll()
    }

    fn has_in_flight(&self) -> bool {
        !self.in_flight.is_empty()
    }
}

impl<T, S, E> Task for Server<S, T>
    where T: Transport<Error = E>,
          S: ServerService<Req = T::Out, Resp = T::In, Body = T::BodyIn, Error = E>,
          E: From<Error<E>> + Send + 'static,
{
    fn tick(&mut self) -> io::Result<Tick> {
        self.inner.tick()
    }
}
