//! An "easy" multiplexing module
//!
//! This module is the same as the top-level `multiplex` module in this crate
//! except that it has no support for streaming bodies. This in turn simplifies
//! a number of generics and allows for protocols to easily get off the ground
//! running.
//!
//! The API surface area of this module is the same as the top-level one, so no
//! new functionality is introduced here. In fact all APIs here are built on top
//! of the `multiplex` module itself, they just simplify the generics in play and
//! such.

use std::io;
use std::marker;

use futures::stream::Receiver;
use futures::{Future, Async, Poll};
use tokio_core::io::FramedIo;
use tokio_core::reactor::Handle;
use tokio_service::Service;

use easy::EasyClient;
use multiplex::{self, RequestId};
use {Message, Body};

/// The "easy" form of connecting a multiplexed client.
///
/// This function takes an instance of the `tokio_core::io::FrameIo` trait as
/// the `frames` argument, a `handle` to the event loop to connect on, and then
/// returns the connected client.
///
/// The client returned implements the `Service` trait. This trait
/// implementation allows sending requests to the transport provided and returns
/// futures to the responses. All requests are automatically multiplexed and
/// managed internally.
pub fn connect<F, T>(frames: F, handle: &Handle)
    -> EasyClient<F::In, T>
    where F: FramedIo<Out = Option<(RequestId, T)>> + 'static,
          F::In: 'static,
          T: 'static,
{
    EasyClient {
        inner: multiplex::connect(MyTransport::new(frames), handle),
    }
}

struct MyTransport<F, T> {
    inner: F,
    _marker: marker::PhantomData<fn() -> T>,
}

impl<F, T> MyTransport<F, T> {
    fn new(f: F) -> MyTransport<F, T> {
        MyTransport {
            inner: f,
            _marker: marker::PhantomData,
        }
    }
}

impl<F, T> multiplex::Transport for MyTransport<F, T>
    where F: FramedIo<Out = Option<(RequestId, T)>> + 'static,
          F::In: 'static,
          T: 'static,
{
    type In = F::In;
    type BodyIn = ();
    type Out = T;
    type BodyOut = ();
    type Error = io::Error;

    fn poll_read(&mut self) -> Async<()> {
        self.inner.poll_read()
    }

    fn read(&mut self) -> Poll<multiplex::Frame<T, (), io::Error>, io::Error> {
        let (id, msg) = match try_ready!(self.inner.read()) {
            Some(msg) => msg,
            None => return Ok(multiplex::Frame::Done.into()),
        };
        Ok(multiplex::Frame::Message {
            message: msg,
            body: false,
            solo: false,
            id: id,
        }.into())
    }

    fn poll_write(&mut self) -> Async<()> {
        self.inner.poll_write()
    }

    fn write(&mut self, req: multiplex::Frame<F::In, (), io::Error>) -> Poll<(), io::Error> {
        match req {
            multiplex::Frame::Message { message, .. } => self.inner.write(message),
            _ => panic!("not a message frame"),
        }
    }

    fn flush(&mut self) -> Poll<(), io::Error> {
        self.inner.flush()
    }
}

/// An "easy" multiplexed server.
///
/// This struct is an implementation of a server which takes a `FramedIo` (`T`)
/// and dispatches all requests to a service (`S`) to be handled. Internally
/// requests are multiplexed and dispatched/written appropriately over time.
///
/// Note that no streaming request/response bodies are supported with this
/// server to help simplify generics and get off the ground running. If
/// streaming bodies are desired then the top level `multiplex::Server` type can
/// be used (which this is built on).
pub struct EasyServer<S, T, I>
    where S: Service<Request = I, Response = T::In, Error = io::Error> + 'static,
          T: FramedIo<Out = Option<(RequestId, I)>> + 'static,
          T::In: 'static,
          I: 'static,
{
    inner: multiplex::Server<MyService<S>,
                             MyTransport<T, I>,
                             Receiver<(), io::Error>>,
}

impl<S, T, I> EasyServer<S, T, I>
    where S: Service<Request = I, Response = T::In, Error = io::Error> + 'static,
          T: FramedIo<Out = Option<(RequestId, I)>> + 'static,
          T::In: 'static,
          I: 'static,
{
    /// Instantiates a new multiplexed server.
    ///
    /// The returned server implements the `Future` trait and is used to
    /// internally drive forward all requests for this given transport. The
    /// `transport` provided is an instance of `FramedIo` where the requests are
    /// dispatched to the `service` provided to get a response to write.
    pub fn new(service: S, transport: T) -> EasyServer<S, T, I> {
        EasyServer {
            inner: multiplex::Server::new(MyService(service),
                                          MyTransport::new(transport)),
        }
    }
}

impl<S, T, I> Future for EasyServer<S, T, I>
    where S: Service<Request = I, Response = T::In, Error = io::Error> + 'static,
          T: FramedIo<Out = Option<(RequestId, I)>> + 'static,
          T::In: 'static,
          I: 'static,
{
    type Item = ();
    type Error = ();

    fn poll(&mut self) -> Poll<(), ()> {
        self.inner.poll()
    }
}

struct MyService<S>(S);

impl<S: Service> Service for MyService<S> {
    type Request = Message<S::Request, Body<(), io::Error>>;
    type Response = Message<S::Response, Receiver<(), S::Error>>;
    type Error = S::Error;
    type Future = MyFuture<S::Future, Receiver<(), S::Error>>;

    fn call(&self, req: Self::Request) -> Self::Future {
        match req {
            Message::WithoutBody(msg) => {
                MyFuture(self.0.call(msg), marker::PhantomData)
            }
            Message::WithBody(..) => panic!("bodies not supported"),
        }
    }

    fn poll_ready(&self) -> Async<()> {
        self.0.poll_ready()
    }
}

struct MyFuture<F, T>(F, marker::PhantomData<fn() -> T>);

impl<F: Future, T> Future for MyFuture<F, T> {
    type Item = Message<F::Item, T>;
    type Error = F::Error;

    fn poll(&mut self) -> Poll<Self::Item, Self::Error> {
        let item = try_ready!(self.0.poll());
        Ok(Message::WithoutBody(item).into())
    }
}
