//! Infrastructure for testing the multiplexer
//!
//! The protocol is organized as follows:
//!
//! * message head: &'static str
//! * message body chunk: u32
//! * message body: stream of u32

extern crate tokio_service;

use tokio_proto::Message;
use tokio_proto::multiplex::{self as proto, RequestId};
use tokio_core::reactor::{Core};
use self::tokio_service::{Service, simple_service};

use support::mock;

use futures::{Future, oneshot};
use futures::stream::Stream;
use std::{io, thread};
use std::sync::{mpsc};

/// Message head
pub type Head = &'static str;

/// Streaming body chunk
pub type Chunk = u32;

/// Streaming body
pub type Body = ::tokio_proto::Body<Chunk, io::Error>;

/// Streaming response type
pub type BodyBox = Box<Stream<Item = Chunk, Error = io::Error> + Send + 'static>;

/// Protocol frame
pub type Frame = ::tokio_proto::multiplex::Frame<Head, Chunk, io::Error>;

/// Client handle
pub type Client = ::tokio_proto::Client<Head, Head, Body, Body, io::Error>;

/// Transport handle
pub type TransportHandle = mock::TransportHandle<Frame, Frame>;

/// Return a message frame without a body
pub fn message(id: RequestId, msg: Head) -> Frame {
    proto::Frame::Message {
        id: id,
        message: msg,
        body: false,
        solo: false,
    }
}

/// Return a message frame with a body
pub fn message_with_body(id: RequestId, message: Head) -> Frame {
    proto::Frame::Message {
        id: id,
        message: message,
        body: true,
        solo: false,
    }
}

/// Return a body frame
pub fn body(id: RequestId, chunk: Option<Chunk>) -> Frame {
    proto::Frame::Body {
        id: id,
        chunk: chunk,
    }
}

pub fn error(id: RequestId, error: io::Error) -> Frame {
    proto::Frame::Error {
        id: id,
        error: error,
    }
}

pub fn done() -> Frame {
    proto::Frame::Done
}

pub trait NewTransport: Send + 'static {
    type Transport: proto::Transport<In = Head,
                                    Out = Head,
                                 BodyIn = Chunk,
                                BodyOut = Chunk,
                                  Error = io::Error> + Send;

    fn new_transport(self, transport: mock::Transport<Frame, Frame>) -> Self::Transport;
}

/// Setup a reactor running a multiplex::Server with the given service and a
/// mock transport. Yields the mock transport handle to the function.
pub fn run<S, F>(service: S, f: F)
    where S: Service<Request = Message<Head, Body>,
                    Response = Message<Head, BodyBox>,
                       Error = io::Error> + Sync + Send + 'static,
          S::Future: Send + 'static,
          F: FnOnce(mock::TransportHandle<Frame, Frame>)
{
    let service = simple_service(move |request| {
        Box::new(service.call(request)) as Box<Future<Item = Message<Head, BodyBox>, Error = io::Error> + Send + 'static>
    });

    _run(Box::new(service), |t| t, f);
}

/// Setup a reactor running a multiplex::Server with the given service and a
/// mock transport. Yields the mock transport handle to the function.
pub fn run_with_transport<S, F, T>(service: S, new_transport: T, f: F)
    where S: Service<Request = Message<Head, Body>,
                    Response = Message<Head, BodyBox>,
                       Error = io::Error> + Sync + Send + 'static,
          S::Future: Send + 'static,
          T: NewTransport,
          F: FnOnce(mock::TransportHandle<Frame, Frame>)
{
    let service = simple_service(move |request| {
        Box::new(service.call(request)) as Box<Future<Item = Message<Head, BodyBox>, Error = io::Error> + Send + 'static>
    });

    _run(Box::new(service), new_transport, f);
}

/// Setup a reactor running a multiplex::Client and a mock transport. Yields the
/// mock transport handle to the function.
pub fn client<F>(f: F) where F: FnOnce(TransportHandle, Client) {
    let _ = ::env_logger::init();

    let (tx, rx) = oneshot();
    let (tx2, rx2) = mpsc::channel();
    let t = thread::spawn(move || {
        let mut lp = Core::new().unwrap();
        let handle = lp.handle();
        let (mock, new_transport) = mock::transport::<Frame, Frame>(handle.clone());

        let transport = new_transport.new_transport().unwrap();
        let service = proto::connect(transport, &handle);

        tx2.send((mock, service)).unwrap();
        lp.run(rx)
    });

    let (mock, service) = rx2.recv().unwrap();

    f(mock, service);

    tx.complete(());
    t.join().unwrap().unwrap();
}

type ServerService = Box<Service<Request = Message<Head, Body>,
                                Response = Message<Head, BodyBox>,
                                   Error = io::Error,
                                  Future = Box<Future<Item = Message<Head, BodyBox>, Error = io::Error> + Send + 'static>> + Send + 'static>;

type BoxTransport = Box<proto::Transport<In = Head,
                                         Out = Head,
                                         BodyIn = Chunk,
                                         BodyOut = Chunk,
                                         Error = io::Error> + Send>;

// Convert to trait objects in a hope to make compiling tests faster
fn _run<F, T>(service: ServerService,
              new_transport: T,
              f: F)
    where F: FnOnce(TransportHandle),
          T: NewTransport,
{
    let _ = ::env_logger::init();

    let (tx, rx) = oneshot();
    let (tx2, rx2) = mpsc::channel();
    let t = thread::spawn(move || {
        let mut lp = Core::new().unwrap();
        let handle = lp.handle();

        let (mock, new_mock) = mock::transport::<Frame, Frame>(handle.clone());
        let transport = new_mock.new_transport().unwrap();
        let transport = new_transport.new_transport(transport);
        let transport: BoxTransport = Box::new(transport);

        handle.spawn(proto::Server::new(service, transport));

        tx2.send(mock).unwrap();
        lp.run(rx)
    });
    let mock = rx2.recv().unwrap();

    f(mock);

    tx.complete(());
    t.join().unwrap().unwrap();
}

impl<F, T> NewTransport for F
    where F: FnOnce(mock::Transport<Frame, Frame>) -> T + Send + 'static,
          T: proto::Transport<In = Head,
                             Out = Head,
                          BodyIn = Chunk,
                         BodyOut = Chunk,
                           Error = io::Error> + Send,
{
    type Transport = T;

    fn new_transport(self, mock: mock::Transport<Frame, Frame>) -> T {
        self(mock)
    }
}
