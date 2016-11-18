use BindClient;
use super::Pipeline;
use super::lift::{LiftBind, LiftTransport};
use simple::LiftProto;

use streaming::{self, Message};
use streaming::pipeline::StreamingPipeline;
use tokio_core::reactor::Handle;
use tokio_service::Service;
use transport::Transport;
use futures::{stream, Future, Poll};
use std::io;

type MyStream<E> = stream::Empty<(), E>;

/// A pipelined client protocol.
///
/// The `T` parameter is used for the I/O object used to communicate, which is
/// supplied in `bind_transport`.
///
/// For simple protocols, the `Self` type is often a unit struct. In more
/// advanced cases, `Self` may contain configuration information that is used
/// for setting up the transport in `bind_transport`.
pub trait ClientProto<T: 'static>: 'static {
    /// Request messages.
    type Request: 'static;

    /// Response messages.
    type Response: 'static;

    /// Errors produced by the service.
    type Error: From<io::Error> + 'static;

    /// The message transport, which usually take `T` as a parameter.
    type Transport: Transport<T, ReadFrame = Self::Response, WriteFrame = Self::Request>;

    /// Build a transport from the given I/O object, using `self` for any
    /// configuration.
    fn bind_transport(&self, io: T) -> <Self::Transport as Transport<T>>::Bind {
        <Self::Transport as Transport<T>>::bind(io)
    }
}

impl<T: 'static, P: ClientProto<T>> BindClient<Pipeline, T> for P {
    type ServiceRequest = P::Request;
    type ServiceResponse = P::Response;
    type ServiceError = P::Error;

    type BindClient = ClientService<T, P>;

    fn bind_client(&self, handle: &Handle, io: T) -> Self::BindClient {
        ClientService {
            inner: BindClient::<StreamingPipeline<MyStream<P::Error>>, T>::bind_client(
                LiftProto::from_ref(self), handle, io
            )
        }
    }
}

impl<T, P> streaming::pipeline::ClientProto<T> for LiftProto<P> where
    T: 'static, P: ClientProto<T>
{
    type Request = P::Request;
    type RequestBody = ();

    type Response = P::Response;
    type ResponseBody = ();

    type Error = P::Error;

    type Transport = LiftTransport<P::Transport, P::Error>;

    fn bind_transport(&self, io: T) -> <Self::Transport as Transport<T>>::Bind {
        LiftBind::lift(ClientProto::bind_transport(self.lower(), io))
    }
}

pub struct ClientService<T, P> where T: 'static, P: ClientProto<T> {
    inner: <LiftProto<P> as BindClient<StreamingPipeline<MyStream<P::Error>>, T>>::BindClient
}

impl<T, P> Service for ClientService<T, P> where T: 'static, P: ClientProto<T> {
    type Request = P::Request;
    type Response = P::Response;
    type Error = P::Error;
    type Future = ClientFuture<T, P>;

    fn call(&self, req: P::Request) -> Self::Future {
        ClientFuture {
            inner: self.inner.call(Message::WithoutBody(req))
        }
    }
}

pub struct ClientFuture<T, P> where T: 'static, P: ClientProto<T> {
    inner: <<LiftProto<P> as BindClient<StreamingPipeline<MyStream<P::Error>>, T>>::BindClient
            as Service>::Future
}

impl<T, P> Future for ClientFuture<T, P> where P: ClientProto<T> {
    type Item = P::Response;
    type Error = P::Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        match try_ready!(self.inner.poll()) {
            Message::WithoutBody(msg) => Ok(msg.into()),
            Message::WithBody(..) => panic!("bodies not supported"),
        }
    }
}
