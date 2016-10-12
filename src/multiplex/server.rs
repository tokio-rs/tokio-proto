use {Message, Body};
use super::{multiplex, RequestId, Transport, Multiplex, MultiplexMessage};
use tokio_service::Service;
use futures::{Future, Poll, Async};
use futures::stream::Stream;
use std::io;

/// A server `Task` that dispatches `Transport` messages to a `Service` using
/// protocol multiplexing.
pub struct Server<S, T, B>
    where T: Transport,
          S: Service<Request = Message<T::Out, Body<T::BodyOut, T::Error>>,
                    Response = Message<T::In, B>,
                       Error = T::Error> + 'static,
          B: Stream<Item = T::BodyIn, Error = T::Error> + 'static,
{
    inner: Multiplex<Dispatch<S, T>>,
}

struct Dispatch<S, T> where S: Service {
    // The service handling the connection
    service: S,
    transport: T,
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
            in_flight: vec![],
        };

        // Create the multiplexer
        let multiplex = Multiplex::new(dispatch);

        // Return the server task
        Server { inner: multiplex }
    }
}

impl<S, T, B> multiplex::Dispatch for Dispatch<S, T>
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

    fn poll(&mut self) -> Poll<Option<MultiplexMessage<Self::In, B, Self::Error>>, io::Error> {
        trace!("Dispatch::poll");

        let mut idx = None;

        for (i, &mut (request_id, ref mut slot)) in self.in_flight.iter_mut().enumerate() {
            trace!("   --> poll; request_id={:?}", request_id);
            if slot.poll() && idx.is_none() {
                idx = Some(i);
            }
        }

        if let Some(idx) = idx {
            // let (request_id, message) = self.in_flight.remove(idx);
            let (request_id, message) = self.in_flight.remove(idx);
            let message = MultiplexMessage {
                id: request_id,
                message: message.unwrap_done(),
                solo: false,
            };

            Ok(Async::Ready(Some(message)))
        } else {
            Ok(Async::NotReady)
        }
    }

    fn dispatch(&mut self, message: MultiplexMessage<Self::Out, Body<Self::BodyOut, Self::Error>, Self::Error>) -> io::Result<()> {
        assert!(self.poll_ready().is_ready());

        let MultiplexMessage { id, message, solo } = message;

        assert!(!solo);

        if let Ok(request) = message {
            let response = self.service.call(request);
            self.in_flight.push((id, InFlight::Active(response)));
        }

        // TODO: Should the error be handled differently?

        Ok(())
    }

    fn poll_ready(&self) -> Async<()> {
        if self.in_flight.len() < MAX_IN_FLIGHT_REQUESTS {
            Async::Ready(())
        } else {
            Async::NotReady
        }
    }

    fn cancel(&mut self, _request_id: RequestId) -> io::Result<()> {
        // TODO: implement
        Ok(())
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

/*
 *
 * ===== InFlight =====
 *
 */

impl<F> InFlight<F>
    where F: Future,
{
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
