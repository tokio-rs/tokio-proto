use BindClient;
use streaming::{Body, Message};
use super::{StreamingPipeline, Frame, Transport};
use super::advanced::{Pipeline, PipelineMessage};
use util::client_proxy::{self, ClientProxy, Receiver};
use futures::{Future, IntoFuture, Poll, Async, Stream};
use futures::sync::oneshot;
use tokio_core::reactor::Handle;
use std::collections::VecDeque;
use std::io;

/// A streaming, pipelined client protocol.
///
/// The `T` parameter is used for the I/O object used to communicate, which is
/// supplied in `bind_transport`.
///
/// For simple protocols, the `Self` type is often a unit struct. In more
/// advanced cases, `Self` may contain configuration information that is used
/// for setting up the transport in `bind_transport`.
pub trait ClientProto<T: 'static>: 'static {
    /// The type of request headers.
    type Request: 'static;

    /// The type of request body chunks.
    type RequestBody: 'static;

    /// The type of response headers.
    type Response: 'static;

    /// The type of response body chunks.
    type ResponseBody: 'static;

    /// The type of error frames.
    type Error: From<io::Error> + 'static;

    /// The frame transport, which usually take `T` as a parameter.
    type Transport:
        Transport<Item = Frame<Self::Response, Self::ResponseBody, Self::Error>,
                  SinkItem = Frame<Self::Request, Self::RequestBody, Self::Error>>;

    /// A future for initializing a transport from an I/O object.
    ///
    /// In simple cases, `Result<Self::Transport, Self::Error>` often suffices.
    type BindTransport: IntoFuture<Item = Self::Transport, Error = io::Error>;

    /// Build a transport from the given I/O object, using `self` for any
    /// configuration.
    fn bind_transport(&self, io: T) -> Self::BindTransport;
}

impl<P, T, B> BindClient<StreamingPipeline<B>, T> for P where
    P: ClientProto<T>,
    T: 'static,
    B: Stream<Item = P::RequestBody, Error = P::Error> + 'static,
{
    type ServiceRequest = Message<P::Request, B>;
    type ServiceResponse = Message<P::Response, Body<P::ResponseBody, P::Error>>;
    type ServiceError = P::Error;

    type BindClient = ClientProxy<Self::ServiceRequest, Self::ServiceResponse, Self::ServiceError>;

    fn bind_client(&self, handle: &Handle, io: T) -> Self::BindClient {
        let (client, rx) = client_proxy::pair();

        let task = self.bind_transport(io).into_future().and_then(|transport| {
            let dispatch: Dispatch<P, T, B> = Dispatch {
                transport: transport,
                requests: rx,
                in_flight: VecDeque::with_capacity(32),
            };
            Pipeline::new(dispatch)
        }).map_err(|e| {
            // TODO: where to punt this error to?
            error!("pipeline error: {}", e);
        });

        // Spawn the task
        handle.spawn(task);

        // Return the client
        client
    }
}

struct Dispatch<P, T, B> where
    P: ClientProto<T> + BindClient<StreamingPipeline<B>, T>,
    T: 'static,
    B: Stream<Item = P::RequestBody, Error = P::Error> + 'static,
{
    transport: P::Transport,
    requests: Receiver<P::ServiceRequest, P::ServiceResponse, P::Error>,
    in_flight: VecDeque<oneshot::Sender<Result<P::ServiceResponse, P::Error>>>,
}

impl<P, T, B> super::advanced::Dispatch for Dispatch<P, T, B> where
    P: ClientProto<T>,
    B: Stream<Item = P::RequestBody, Error = P::Error>,
{
    type Io = T;
    type In = P::Request;
    type BodyIn = P::RequestBody;
    type Out = P::Response;
    type BodyOut = P::ResponseBody;
    type Error = P::Error;
    type Stream = B;
    type Transport = P::Transport;

    fn transport(&mut self) -> &mut Self::Transport {
        &mut self.transport
    }

    fn dispatch(&mut self,
                response: PipelineMessage<Self::Out, Body<Self::BodyOut, Self::Error>, Self::Error>)
                -> io::Result<()>
    {
        if let Some(complete) = self.in_flight.pop_front() {
            drop(complete.send(response));
        } else {
            return Err(io::Error::new(io::ErrorKind::Other, "request / response mismatch"));
        }

        Ok(())
    }

    fn poll(&mut self) -> Poll<Option<PipelineMessage<Self::In, Self::Stream, Self::Error>>,
                               io::Error>
    {
        trace!("Dispatch::poll");
        // Try to get a new request frame
        match self.requests.poll() {
            Ok(Async::Ready(Some(Ok((request, complete))))) => {
                trace!("   --> received request");

                // Track complete handle
                self.in_flight.push_back(complete);

                Ok(Async::Ready(Some(Ok(request))))

            }
            Ok(Async::Ready(None)) => {
                trace!("   --> client dropped");
                Ok(Async::Ready(None))
            }
            Ok(Async::Ready(Some(Err(e)))) => {
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
            Err(()) => panic!(),
        }
    }

    fn has_in_flight(&self) -> bool {
        !self.in_flight.is_empty()
    }
}

impl<P, T, B> Drop for Dispatch<P, T, B> where
    P: ClientProto<T> + BindClient<StreamingPipeline<B>, T>,
    T: 'static,
    B: Stream<Item = P::RequestBody, Error = P::Error> + 'static,
{
    fn drop(&mut self) {
        // Complete any pending requests with an error
        while let Some(complete) = self.in_flight.pop_front() {
            drop(complete.send(Err(broken_pipe().into())));
        }
    }
}

fn broken_pipe() -> io::Error {
    io::Error::new(io::ErrorKind::BrokenPipe, "broken pipe")
}
