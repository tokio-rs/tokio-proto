use BindServer;
use futures::stream::Stream;
use futures::{Future, IntoFuture, Poll, Async};
use std::collections::VecDeque;
use std::io;
use streaming::{Message, Body};
use super::advanced::{Pipeline, PipelineMessage};
use super::{Frame, Transport};
use tokio_core::reactor::Handle;
use tokio_service::Service;

// TODO:
//
// - Wait for service readiness
// - Handle request body stream cancellation

/// A streaming, pipelined server protocol.
///
/// The `T` parameter is used for the I/O object used to communicate, which is
/// supplied in `bind_transport`.
///
/// For simple protocols, the `Self` type is often a unit struct. In more
/// advanced cases, `Self` may contain configuration information that is used
/// for setting up the transport in `bind_transport`.
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
        Transport<Item = Frame<Self::Request, Self::RequestBody, Self::Error>,
                  SinkItem = Frame<Self::Response, Self::ResponseBody, Self::Error>>;

    /// A future for initializing a transport from an I/O object.
    ///
    /// In simple cases, `Result<Self::Transport, Self::Error>` often suffices.
    type BindTransport: IntoFuture<Item = Self::Transport, Error = io::Error>;

    /// Build a transport from the given I/O object, using `self` for any
    /// configuration.
    fn bind_transport(&self, io: T) -> Self::BindTransport;
}

impl<P, T, B> BindServer<super::StreamingPipeline<B>, T> for P where
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
                in_flight: VecDeque::with_capacity(32),
            };
            Pipeline::new(dispatch)
        });

        // Spawn the pipeline dispatcher
        handle.spawn(task.map_err(|_| ()))
    }
}

struct Dispatch<S, T, P> where
    T: 'static, P: ServerProto<T>, S: Service
{
    // The service handling the connection
    service: S,
    transport: P::Transport,
    in_flight: VecDeque<InFlight<S::Future>>,
}

enum InFlight<F: Future> {
    Active(F),
    Done(Result<F::Item, F::Error>),
}

impl<P, T, B, S> super::advanced::Dispatch for Dispatch<S, T, P> where
    P: ServerProto<T>,
    T: 'static,
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
