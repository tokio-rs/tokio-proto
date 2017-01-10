use std::io;
use std::marker;

use BindServer;
use super::{RequestId, Multiplex};
use super::lift::{LiftBind, LiftTransport};
use simple::LiftProto;

use streaming::{self, Message};
use streaming::multiplex::StreamingMultiplex;
use tokio_core::reactor::Handle;
use tokio_service::Service;
use futures::{stream, Stream, Sink, Future, IntoFuture, Poll};

type MyStream<E> = stream::Empty<(), E>;

/// An multiplexed server protocol.
///
/// The `T` parameter is used for the I/O object used to communicate, which is
/// supplied in `bind_transport`.
///
/// For simple protocols, the `Self` type is often a unit struct. In more
/// advanced cases, `Self` may contain configuration information that is used
/// for setting up the transport in `bind_transport`.
pub trait ServerProto<T: 'static>: 'static {
    /// Request messages.
    type Request: 'static;

    /// Response messages.
    type Response: 'static;

    /// The message transport, which usually take `T` as a parameter.
    ///
    /// An easy way to build a transport is to use `tokio_core::io::Framed`
    /// together with a `Codec`; in that case, the transport type is
    /// `Framed<T, YourCodec>`. See the crate docs for an example.
    type Transport: 'static +
        Stream<Item = (RequestId, Self::Request), Error = io::Error> +
        Sink<SinkItem = (RequestId, Self::Response), SinkError = io::Error>;

    /// A future for initializing a transport from an I/O object.
    ///
    /// In simple cases, `Result<Self::Transport, Self::Error>` often suffices.
    type BindTransport: IntoFuture<Item = Self::Transport, Error = io::Error>;

    /// Build a transport from the given I/O object, using `self` for any
    /// configuration.
    ///
    /// An easy way to build a transport is to use `tokio_core::io::Framed`
    /// together with a `Codec`; in that case, `bind_transport` is just
    /// `io.framed(YourCodec)`. See the crate docs for an example.
    fn bind_transport(&self, io: T) -> Self::BindTransport;
}

impl<T: 'static, P: ServerProto<T>> BindServer<Multiplex, T> for P {
    type ServiceRequest = P::Request;
    type ServiceResponse = P::Response;
    type ServiceError = io::Error;

    fn bind_server<S>(&self, handle: &Handle, io: T, service: S)
        where S: Service<Request = Self::ServiceRequest,
                         Response = Self::ServiceResponse,
                         Error = Self::ServiceError> + 'static
    {
        BindServer::<StreamingMultiplex<MyStream<io::Error>>, T>::bind_server(
            LiftProto::from_ref(self), handle, io, LiftService(service)
        )
    }
}

impl<T, P> streaming::multiplex::ServerProto<T> for LiftProto<P> where
    T: 'static, P: ServerProto<T>
{
    type Request = P::Request;
    type RequestBody = ();

    type Response = P::Response;
    type ResponseBody = ();

    type Error = io::Error;

    type Transport = LiftTransport<P::Transport, io::Error>;
    type BindTransport = LiftBind<T, <P::BindTransport as IntoFuture>::Future, io::Error>;

    fn bind_transport(&self, io: T) -> Self::BindTransport {
        LiftBind::lift(ServerProto::bind_transport(self.lower(), io).into_future())
    }
}

struct LiftService<S>(S);

impl<S: Service> Service for LiftService<S> {
    type Request = streaming::Message<S::Request, streaming::Body<(), S::Error>>;
    type Response = streaming::Message<S::Response, MyStream<S::Error>>;
    type Error = S::Error;
    type Future = LiftFuture<S::Future, stream::Empty<(), S::Error>>;

    fn call(&self, req: Self::Request) -> Self::Future {
        match req {
            Message::WithoutBody(msg) => {
                LiftFuture(self.0.call(msg), marker::PhantomData)
            }
            Message::WithBody(..) => panic!("bodies not supported"),
        }
    }
}

struct LiftFuture<F, T>(F, marker::PhantomData<fn() -> T>);

impl<F: Future, T> Future for LiftFuture<F, T> {
    type Item = Message<F::Item, T>;
    type Error = F::Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let item = try_ready!(self.0.poll());
        Ok(Message::WithoutBody(item).into())
    }
}
