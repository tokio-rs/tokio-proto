use super::{Frame, RequestId, StreamingMultiplex, Transport};
use super::advanced::{Multiplex, MultiplexMessage};

use BindClient;
use streaming::{Body, Message};
use util::client_proxy::{self, ClientProxy, Receiver};
use futures::{Future, IntoFuture, Poll, Async, Stream};
use futures::sync::oneshot;
use tokio_core::reactor::Handle;
use std::io;
use std::collections::HashMap;

/// A streaming, multiplexed client protocol.
///
/// The `T` parameter is used for the I/O object used to communicate, which is
/// supplied in `bind_transport`.
///
/// For simple protocols, the `Self` type is often a unit struct. In more
/// advanced cases, `Self` may contain configuration information that is used
/// for setting up the transport in `bind_transport`.
pub trait ClientProto<T: 'static>: 'static {
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
        Transport<Self::ResponseBody,
                  Item = Frame<Self::Response, Self::ResponseBody, Self::Error>,
                  SinkItem = Frame<Self::Request, Self::RequestBody, Self::Error>>;

    /// A future for initializing a transport from an I/O object.
    ///
    /// In simple cases, `Result<Self::Transport, Self::Error>` often suffices.
    type BindTransport: IntoFuture<Item = Self::Transport, Error = io::Error>;

    /// Build a transport from the given I/O object, using `self` for any
    /// configuration.
    fn bind_transport(&self, io: T) -> Self::BindTransport;
}

impl<P, T, B> BindClient<StreamingMultiplex<B>, T> for P where
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
                in_flight: HashMap::new(),
                next_request_id: 0,
            };
            Multiplex::new(dispatch)
        }).map_err(|e| {
            // TODO: where to punt this error to?
            debug!("multiplex task failed with error; err={:?}", e);
        });

        // Spawn the task
        handle.spawn(task);

        // Return the client
        client
    }
}

struct Dispatch<P, T, B> where
    P: ClientProto<T> + BindClient<StreamingMultiplex<B>, T>,
    T: 'static,
    B: Stream<Item = P::RequestBody, Error = P::Error> + 'static,
{
    transport: P::Transport,
    requests: Receiver<P::ServiceRequest, P::ServiceResponse, P::Error>,
    in_flight: HashMap<RequestId, oneshot::Sender<Result<P::ServiceResponse, P::Error>>>,
    next_request_id: u64,
}

impl<P, T, B> super::advanced::Dispatch for Dispatch<P, T, B> where
    P: ClientProto<T>,
    T: 'static,
    B: Stream<Item = P::RequestBody, Error = P::Error> + 'static,
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

    fn dispatch(&mut self, message: MultiplexMessage<Self::Out, Body<Self::BodyOut, Self::Error>, Self::Error>) -> io::Result<()> {
        let MultiplexMessage { id, message, solo } = message;

        assert!(!solo);

        if let Some(complete) = self.in_flight.remove(&id) {
            drop(complete.send(message));
        } else {
            return Err(io::Error::new(io::ErrorKind::Other, "request / response mismatch"));
        }

        Ok(())
    }

    fn poll(&mut self) -> Poll<Option<MultiplexMessage<Self::In, B, Self::Error>>, io::Error> {
        trace!("Dispatch::poll");
        // Try to get a new request frame
        match self.requests.poll() {
            Ok(Async::Ready(Some(Ok((request, complete))))) => {
                trace!("   --> received request");

                let request_id = self.next_request_id;
                self.next_request_id += 1;

                trace!("   --> assigning request-id={:?}", request_id);

                // Track complete handle
                self.in_flight.insert(request_id, complete);

                Ok(Async::Ready(Some(MultiplexMessage::new(request_id, request))))

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

    fn poll_ready(&self) -> Async<()> {
        // Not capping the client yet
        Async::Ready(())
    }

    fn cancel(&mut self, _request_id: RequestId) -> io::Result<()> {
        // TODO: implement
        Ok(())
    }
}

impl<P, T, B> Drop for Dispatch<P, T, B> where
    P: ClientProto<T> + BindClient<StreamingMultiplex<B>, T>,
    T: 'static,
    B: Stream<Item = P::RequestBody, Error = P::Error> + 'static,
{
    fn drop(&mut self) {
        if !self.in_flight.is_empty() {
            warn!("multiplex client dropping with in-flight exchanges");
        }

        // Complete any pending requests with an error
        for (_, complete) in self.in_flight.drain() {
            drop(complete.send(Err(broken_pipe().into())));
        }
    }
}

fn broken_pipe() -> io::Error {
    io::Error::new(io::ErrorKind::BrokenPipe, "broken pipe")
}
