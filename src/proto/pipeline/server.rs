use super::{pipeline, Error, Message, ServerService, Transport};
use std::collections::VecDeque;
use std::io;
use futures::{Future, Poll};

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
    in_flight: VecDeque<InFlight<S::Fut>>,
}

enum InFlight<F: Future> {
    Active(F),
    Done(Result<F::Item, F::Error>),
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
            in_flight: VecDeque::with_capacity(32),
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
        self.in_flight.push_back(InFlight::Active(response));
        Ok(())
    }

    fn poll(&mut self) -> Option<Result<Message<Self::InMsg, Self::InBodyStream>, Self::Error>> {
        for slot in self.in_flight.iter_mut() {
            slot.poll();
        }
        match self.in_flight.front() {
            Some(&InFlight::Done(_)) => {}
            _ => return None,
        }
        match self.in_flight.pop_front() {
            Some(InFlight::Done(res)) => Some(res),
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
                    Poll::Ok(e) => Ok(e),
                    Poll::Err(e) => Err(e),
                    Poll::NotReady => return,
                }
            }
            _ => return,
        };
        *self = InFlight::Done(res);
    }
}

impl<T, S, E> Future for Server<S, T>
    where T: Transport<Error = E>,
          S: ServerService<Req = T::Out, Resp = T::In, Body = T::BodyIn, Error = E>,
          E: From<Error<E>> + Send + 'static,
{
    type Item = ();
    type Error = io::Error;

    fn poll(&mut self) -> Poll<(), io::Error> {
        self.inner.poll()
    }
}
