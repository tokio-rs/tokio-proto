use {Error, Message};
use super::{multiplex, RequestId, ServerService, Transport};
use futures::{Future, Poll, Async};
use std::io;

/// A server `Task` that dispatches `Transport` messages to a `Service` using
/// protocol multiplexing.
pub struct Server<S, T>
    where S: ServerService,
          T: Transport,
{
    inner: multiplex::Multiplex<Dispatch<S>, T>,
}

struct Dispatch<S: ServerService> {
    // The service handling the connection
    service: S,
    in_flight: Vec<(RequestId, InFlight<S::Future>)>,
}

enum InFlight<F: Future> {
    Active(F),
    Done(Result<F::Item, F::Error>),
}

/// The total number of requests that can be in flight at once.
const MAX_IN_FLIGHT_REQUESTS: usize = 32;

/*
 *
 * ===== Server =====
 *
 */

impl<T, S, E> Server<S, T>
    where T: Transport<Error = E>,
          S: ServerService<Request = T::Out, Response = T::In, Body = T::BodyIn, Error = E>,
          E: From<Error<E>>,
{
    /// Create a new pipeline `Server` dispatcher with the given service and
    /// transport
    pub fn new(service: S, transport: T) -> io::Result<Server<S, T>> {
        let dispatch = Dispatch {
            service: service,
            in_flight: vec![],
        };

        // Create the multiplexer
        let multiplex = try!(multiplex::Multiplex::new(dispatch, transport));

        // Return the server task
        Ok(Server { inner: multiplex })
    }
}

impl<S> multiplex::Dispatch for Dispatch<S>
    where S: ServerService,
{
    type InMsg = S::Response;
    type InBody = S::Body;
    type InBodyStream = S::BodyStream;
    type OutMsg = S::Request;
    type Error = S::Error;

    fn dispatch(&mut self, request_id: RequestId, request: Self::OutMsg) -> io::Result<()> {
        let response = self.service.call(request);
        self.in_flight.push((request_id, InFlight::Active(response)));
        Ok(())
    }

    fn poll(&mut self) -> Option<(RequestId, Result<Message<Self::InMsg, Self::InBodyStream>, Self::Error>)> {
        trace!("Dispatch::poll");

        let mut idx = None;

        for (i, &mut (request_id, ref mut slot)) in self.in_flight.iter_mut().enumerate() {
            trace!("   --> poll; request_id={:?}", request_id);
            if slot.poll() && idx.is_none() {
                idx = Some(i);
            }
        }

        if let Some(idx) = idx {
            let (request_id, msg) = self.in_flight.remove(idx);
            Some((request_id, msg.unwrap_done()))
        } else {
            None
        }
    }

    fn is_ready(&self) -> bool {
        self.in_flight.len() < MAX_IN_FLIGHT_REQUESTS
    }

    fn has_in_flight(&self) -> bool {
        !self.in_flight.is_empty()
    }
}

impl<T, S, E> Future for Server<S, T>
    where T: Transport<Error = E>,
          S: ServerService<Request = T::Out, Response = T::In, Body = T::BodyIn, Error = E>,
          E: From<Error<E>>,
{
    type Item = ();
    type Error = io::Error;

    fn poll(&mut self) -> Poll<(), io::Error> {
        self.inner.poll()
    }
}

/*
 *
 * ===== InFlight =====
 *
 */

impl<F: Future> InFlight<F> {
    // Returns true if done
    fn poll(&mut self) -> bool {
        let res = match *self {
            InFlight::Active(ref mut f) => {
                trace!("   --> polling future");
                match f.poll() {
                    Ok(Async::Ready(e)) => Ok(e),
                    Err(e) => Err(e),
                    Ok(Async::NotReady) => return false,
                }
            }
            _ => return true,
        };

        *self = InFlight::Done(res);
        true
    }

    fn unwrap_done(self) -> Result<F::Item, F::Error> {
        match self {
            InFlight::Done(res) => res,
            _ => panic!("future is not ready"),
        }
    }
}
