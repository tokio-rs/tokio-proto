use super::{Frame, RequestId, Transport};
use super::advanced::{Multiplex, MultiplexMessage};

use BindServer;
use streaming::{Message, Body};
use tokio_service::Service;
use tokio_core::reactor::Handle;
use futures::{Future, Poll, Async};
use futures::{IntoFuture, Stream};
use std::io;

/// A streaming, multiplexed server protocol.
///
/// The `T` parameter is used for the I/O object used to communicate, which is
/// supplied in `bind_transport`.
///
/// For simple protocols, the `Self` type is often a unit struct. In more
/// advanced cases, `Self` may contain configuration information that is used
/// for setting up the transport in `bind_transport`.
///
/// ## Considerations
///
/// There are some difficulties with implementing back pressure in the case
/// that the wire protocol does not support a means by which backpressure can
/// be signaled to the peer.
///
/// The problem is the potential for deadlock:
///
/// - The server is busy processing requests on this connection, and stops
/// reading frames from its transport.
///
/// - Meanwhile, the processing logic is blocked waiting for another frame that
/// is currently pending on the socket.
///
/// To deal with this, once the connection's frame buffer is filled, a timeout
/// is set. If no further frames are able to be read before the timeout expires,
/// the connection is killed.
pub trait ServerProto<T: 'static>: 'static {
    /// Request headers.
    type Request: 'static;

    /// Request body chunks.
    type RequestBody: 'static;

    /// Response headers.
    type Response: 'static;

    /// Response body chunks.
    type ResponseBody: 'static;

    /// Errors, which are used both for error frames and for the service itself.
    type Error: From<io::Error> + 'static;

    /// The frame transport, which usually take `T` as a parameter.
    type Transport:
        Transport<Self::RequestBody,
                  Item = Frame<Self::Request, Self::RequestBody, Self::Error>,
                  SinkItem = Frame<Self::Response, Self::ResponseBody, Self::Error>>;

    /// A future for initializing a transport from an I/O object.
    ///
    /// In simple cases, `Result<Self::Transport, Self::Error>` often suffices.
    type BindTransport: IntoFuture<Item = Self::Transport, Error = io::Error>;

    /// Build a transport from the given I/O object, using `self` for any
    /// configuration.
    fn bind_transport(&self, io: T) -> Self::BindTransport;
}

impl<P, T, B> BindServer<super::StreamingMultiplex<B>, T> for P where
    P: ServerProto<T>,
    T: 'static,
    B: Stream<Item = P::ResponseBody, Error = P::Error>,
{
    type ServiceRequest = Message<P::Request, Body<P::RequestBody, P::Error>>;
    type ServiceResponse = Message<P::Response, B>;
    type ServiceError = P::Error;

    fn bind_server<S>(&self, handle: &Handle, io: T, service: S)
        where S: Service<Request = Self::ServiceRequest,
                         Response = Self::ServiceResponse,
                         Error = Self::ServiceError> + 'static
    {
        let task = self.bind_transport(io).into_future().and_then(|transport| {
            let dispatch: Dispatch<S, T, P> = Dispatch {
                service: service,
                transport: transport,
                in_flight: vec![],
            };
            Multiplex::new(dispatch)
        }).map_err(|_| ());

        // Spawn the multiplex dispatcher
        handle.spawn(task)
    }
}

struct Dispatch<S, T, P> where
    T: 'static, P: ServerProto<T>, S: Service
{
    // The service handling the connection
    service: S,
    transport: P::Transport,
    in_flight: Vec<(RequestId, InFlight<S::Future>)>,
}

enum InFlight<F: Future> {
    Active(F),
    Done(Result<F::Item, F::Error>),
}

/// The total number of requests that can be in flight at once.
const MAX_IN_FLIGHT_REQUESTS: usize = 32;

impl<P, T, B, S> super::advanced::Dispatch for Dispatch<S, T, P> where
    P: ServerProto<T>,
    B: Stream<Item = P::ResponseBody, Error = P::Error>,
    S: Service<Request = Message<P::Request, Body<P::RequestBody, P::Error>>,
               Response = Message<P::Response, B>,
               Error = P::Error>,
{
    type Io = T;
    type In = P::Response;
    type BodyIn = P::ResponseBody;
    type Out = P::Request;
    type BodyOut = P::RequestBody;
    type Error = P::Error;
    type Stream = B;
    type Transport = P::Transport;

    fn transport(&mut self) -> &mut P::Transport {
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
