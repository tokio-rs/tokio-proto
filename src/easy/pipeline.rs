//! An "easy" pipelining module
//!
//! This module is the same as the top-level `pipeline` module in this crate
//! except that it has no support for streaming bodies. This in turn simplifies
//! a number of generics and allows for protocols to easily get off the ground
//! running.
//!
//! The API surface area of this module is the same as the top-level one, so no
//! new functionality is introduced here. In fact all APIs here are built on top
//! of the `pipeline` module itself, they just simplify the generics in play and
//! such.

use std::io;
use std::marker;

use futures::stream::Receiver;
use futures::{self, Future, Async, Poll};
use tokio_core::io::FramedIo;
use tokio_core::reactor::Handle;
use tokio_service::Service;

use easy::EasyClient;
use pipeline;
use {Message, Body};

/// The "easy" form of connecting a pipelined client.
///
/// This function takes an instance of the `tokio_core::io::FrameIo` trait as
/// the `frames` argument, a `handle` to the event loop to connect on, and then
/// returns the connected client.
///
/// The client returned implements the `Service` trait. This trait
/// implementation allows sending requests to the transport provided and returns
/// futures to the responses. All requests are automatically pipelined and
/// managed internally.
pub fn connect<F, T>(frames: F, handle: &Handle)
    -> EasyClient<F::In, T>
    where F: FramedIo<Out = Option<T>> + 'static,
          T: 'static,
          F::In: 'static,
{
    let frames = MyTransport(frames, marker::PhantomData);
    EasyClient {
        inner: pipeline::connect(futures::finished(frames), handle),
    }
}

// Lifts an implementation of `FramedIo` to `pipeline::Transport` with no bodies
struct MyTransport<F, T>(F, marker::PhantomData<fn() -> T>);

impl<F, T> pipeline::Transport for MyTransport<F, T>
    where F: FramedIo<Out = Option<T>> + 'static,
          T: 'static,
          F::In: 'static,
{
    type In = F::In;
    type BodyIn = ();
    type Out = T;
    type BodyOut = ();
    type Error = io::Error;

    fn poll_read(&mut self) -> Async<()> {
        self.0.poll_read()
    }

    fn read(&mut self) -> Poll<pipeline::Frame<T, (), io::Error>, io::Error> {
        match try_ready!(self.0.read()) {
            Some(msg) => {
                Ok(pipeline::Frame::Message { message: msg, body: false }.into())
            }
            None => Ok(pipeline::Frame::Done.into()),
        }
    }

    fn poll_write(&mut self) -> Async<()> {
        self.0.poll_write()
    }

    fn write(&mut self, req: pipeline::Frame<F::In, (), io::Error>) -> Poll<(), io::Error> {
        match req {
            pipeline::Frame::Message { message, .. } => self.0.write(message),
            _ => panic!("not a message frame"),
        }
    }

    fn flush(&mut self) -> Poll<(), io::Error> {
        self.0.flush()
    }
}

/// An "easy" pipelined server.
///
/// This struct is an implementation of a server which takes a `FramedIo` (`T`)
/// and dispatches all requests to a service (`S`) to be handled. Internally
/// requests are pipelined and dispatched/written appropriately over time.
///
/// Note that no streaming request/response bodies are supported with this
/// server to help simplify generics and get off the ground running. If
/// streaming bodies are desired then the top level `pipeline::Server` type can
/// be used (which this is built on).
pub struct EasyServer<S, T, I>
    where S: Service<Request = I, Response = T::In, Error = io::Error> + 'static,
          T: FramedIo<Out = Option<I>> + 'static,
          T::In: 'static,
          I: 'static,
{
    inner: pipeline::Server<MyService<S>,
                            MyTransport<T, I>,
                            Receiver<(), io::Error>>,
}

impl<S, T, I> EasyServer<S, T, I>
    where S: Service<Request = I, Response = T::In, Error = io::Error> + 'static,
          T: FramedIo<Out = Option<I>> + 'static,
          T::In: 'static,
          I: 'static,
{
    /// Instantiates a new pipelined server.
    ///
    /// The returned server implements the `Future` trait and is used to
    /// internally drive forward all requests for this given transport. The
    /// `transport` provided is an instance of `FramedIo` where the requests are
    /// dispatched to the `service` provided to get a response to write.
    pub fn new(service: S, transport: T) -> EasyServer<S, T, I> {
        EasyServer {
            inner: pipeline::Server::new(MyService(service),
                                         MyTransport(transport, marker::PhantomData)),
        }
    }
}

impl<S, T, I> Future for EasyServer<S, T, I>
    where S: Service<Request = I, Response = T::In, Error = io::Error> + 'static,
          T: FramedIo<Out = Option<I>> + 'static,
          T::In: 'static,
          I: 'static,
{
    type Item = ();
    type Error = io::Error;

    fn poll(&mut self) -> Poll<(), io::Error> {
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
