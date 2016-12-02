extern crate futures;
extern crate tokio_core;
extern crate tokio_proto;
extern crate tokio_service;
extern crate env_logger;

use std::any::Any;
use std::thread;
use std::cell::RefCell;
use std::io::{self, Read, Write};

use self::futures::stream::Wait;
use self::futures::sync::mpsc;
use self::futures::sync::oneshot;
use self::futures::{Future, Stream, Sink, Poll, StartSend, Async};
use self::tokio_core::io::Io;
use self::tokio_core::reactor::Core;
use self::tokio_proto::streaming::multiplex;
use self::tokio_proto::streaming::pipeline;
use self::tokio_proto::streaming::{Message, Body};
use self::tokio_proto::util::client_proxy::Response;
use self::tokio_proto::{BindClient, BindServer};
use self::tokio_service::Service;

struct MockProtocol<T>(RefCell<Option<MockTransport<T>>>);

impl<T, U, I> pipeline::ClientProto<I> for MockProtocol<pipeline::Frame<T, U, io::Error>>
    where T: 'static,
          U: 'static,
          I: Io + 'static,
{
    type Request = T;
    type RequestBody = U;
    type Response = T;
    type ResponseBody = U;
    type Error = io::Error;
    type Transport = MockTransport<pipeline::Frame<T, U, io::Error>>;
    type BindTransport = Result<Self::Transport, io::Error>;

    fn bind_transport(&self, _io: I)
                      -> Result<MockTransport<pipeline::Frame<T, U, io::Error>>, io::Error> {
        Ok(self.0.borrow_mut().take().unwrap())
    }
}

impl<T, U, I> multiplex::ClientProto<I> for MockProtocol<multiplex::Frame<T, U, io::Error>>
    where T: 'static,
          U: 'static,
          I: Io + 'static,
{
    type Request = T;
    type RequestBody = U;
    type Response = T;
    type ResponseBody = U;
    type Error = io::Error;
    type Transport = MockTransport<multiplex::Frame<T, U, io::Error>>;
    type BindTransport = Result<Self::Transport, io::Error>;

    fn bind_transport(&self, _io: I)
                      -> Result<MockTransport<multiplex::Frame<T, U, io::Error>>, io::Error> {
        Ok(self.0.borrow_mut().take().unwrap())
    }
}

impl<T, U, I> pipeline::ServerProto<I> for MockProtocol<pipeline::Frame<T, U, io::Error>>
    where T: 'static,
          U: 'static,
          I: Io + 'static,
{
    type Request = T;
    type RequestBody = U;
    type Response = T;
    type ResponseBody = U;
    type Error = io::Error;
    type Transport = MockTransport<pipeline::Frame<T, U, io::Error>>;
    type BindTransport = Result<Self::Transport, io::Error>;

    fn bind_transport(&self, _io: I)
                      -> Result<MockTransport<pipeline::Frame<T, U, io::Error>>, io::Error> {
        Ok(self.0.borrow_mut().take().unwrap())
    }
}

impl<T, U, I> multiplex::ServerProto<I> for MockProtocol<multiplex::Frame<T, U, io::Error>>
    where T: 'static,
          U: 'static,
          I: Io + 'static,
{
    type Request = T;
    type RequestBody = U;
    type Response = T;
    type ResponseBody = U;
    type Error = io::Error;
    type Transport = MockTransport<multiplex::Frame<T, U, io::Error>>;
    type BindTransport = Result<Self::Transport, io::Error>;

    fn bind_transport(&self, _io: I)
                      -> Result<MockTransport<multiplex::Frame<T, U, io::Error>>, io::Error> {
        Ok(self.0.borrow_mut().take().unwrap())
    }
}

struct MockTransport<T> {
    tx: mpsc::Sender<T>,
    rx: mpsc::UnboundedReceiver<io::Result<T>>,
}

impl<T: 'static> Stream for MockTransport<T> {
    type Item = T;
    type Error = io::Error;

    fn poll(&mut self) -> Poll<Option<T>, io::Error> {
        match self.rx.poll().expect("rx cannot fail") {
            Async::Ready(Some(Ok(e))) => Ok(Async::Ready(Some(e))),
            Async::Ready(Some(Err(e))) => Err(e),
            Async::Ready(None) => Ok(Async::Ready(None)),
            Async::NotReady => Ok(Async::NotReady),
        }
    }
}

impl<T: 'static> Sink for MockTransport<T> {
    type SinkItem = T;
    type SinkError = io::Error;

    fn start_send(&mut self, item: T) -> StartSend<T, io::Error> {
        Ok(self.tx.start_send(item).expect("should not be closed"))
    }

    fn poll_complete(&mut self) -> Poll<(), io::Error> {
        Ok(self.tx.poll_complete().expect("should not close"))
    }
}

impl<T: 'static> pipeline::Transport for MockTransport<T> {}
impl<B, T: 'static> multiplex::Transport<B> for MockTransport<T> {}

struct MockIo;

impl Read for MockIo {
    fn read(&mut self, _buf: &mut [u8]) -> io::Result<usize> {
        panic!("should not be used")
    }
}

impl Write for MockIo {
    fn write(&mut self, _buf: &[u8]) -> io::Result<usize> {
        panic!("should not be used")
    }

    fn flush(&mut self) -> io::Result<()> {
        panic!("should not be used")
    }
}

impl Io for MockIo {}

pub type MockBodyStream = Box<Stream<Item = u32, Error = io::Error> + Send>;

pub struct MockTransportCtl<T> {
    tx: Option<mpsc::UnboundedSender<io::Result<T>>>,
    rx: Wait<mpsc::Receiver<T>>,
}

impl<T> MockTransportCtl<T> {
    pub fn send(&mut self, msg: T) {
        mpsc::UnboundedSender::send(self.tx.as_mut().unwrap(), Ok(msg))
            .expect("should not be closed");
    }

    pub fn error(&mut self, error: io::Error) {
        mpsc::UnboundedSender::send(self.tx.as_mut().unwrap(), Err(error))
            .expect("should not be closed");
    }

    pub fn next_write(&mut self) -> T {
        self.rx.next().unwrap().expect("cannot error")
    }

    pub fn allow_and_assert_drop(&mut self) {
        drop(self.tx.take());
        assert!(self.rx.next().is_none());
    }
}

fn transport<T>() -> (MockTransportCtl<T>, MockProtocol<T>) {
    let (tx1, rx1) = mpsc::channel(1);
    let (tx2, rx2) = mpsc::unbounded();
    let ctl = MockTransportCtl {
        tx: Some(tx2),
        rx: rx1.wait(),
    };
    let transport = MockTransport {
        tx: tx1,
        rx: rx2,
    };
    (ctl, MockProtocol(RefCell::new(Some(transport))))
}

struct CompleteOnDrop {
    thread: Option<thread::JoinHandle<()>>,
    tx: Option<oneshot::Sender<()>>,
}

impl Drop for CompleteOnDrop {
    fn drop(&mut self) {
        self.tx.take().unwrap().complete(());
        self.thread.take().unwrap().join().unwrap();
    }
}

pub fn pipeline_client()
    -> (MockTransportCtl<pipeline::Frame<&'static str, u32, io::Error>>,
        Box<Service<Request = Message<&'static str, MockBodyStream>,
                    Response = Message<&'static str, Body<u32, io::Error>>,
                    Error = io::Error,
                    Future = Response<Message<&'static str, Body<u32, io::Error>>,
                                              io::Error>>>,
        Box<Any>)
{
    drop(env_logger::init());

    let (ctl, proto) = transport();

    let (tx, rx) = oneshot::channel();
    let (finished_tx, finished_rx) = oneshot::channel();
    let t = thread::spawn(move || {
        let mut core = Core::new().unwrap();
        let handle = core.handle();

        let service = proto.bind_client(&handle, MockIo);
        tx.complete(service);
        drop(core.run(finished_rx));
    });

    let service = rx.wait().unwrap();

    let srv = CompleteOnDrop {
        thread: Some(t),
        tx: Some(finished_tx),
    };
    return (ctl, Box::new(service), Box::new(srv));
}

pub fn pipeline_server<S>(s: S)
    -> (MockTransportCtl<pipeline::Frame<&'static str, u32, io::Error>>, Box<Any>)
    where S: Service<Request = Message<&'static str, Body<u32, io::Error>>,
                     Response = Message<&'static str, MockBodyStream>,
                     Error = io::Error> + Send + 'static,
{
    drop(env_logger::init());

    let (ctl, proto) = transport();

    let (finished_tx, finished_rx) = oneshot::channel();
    let t = thread::spawn(move || {
        let mut core = Core::new().unwrap();
        let handle = core.handle();

        proto.bind_server(&handle, MockIo, s);
        drop(core.run(finished_rx));
    });

    let srv = CompleteOnDrop {
        thread: Some(t),
        tx: Some(finished_tx),
    };
    return (ctl, Box::new(srv));
}

pub fn multiplex_client()
    -> (MockTransportCtl<multiplex::Frame<&'static str, u32, io::Error>>,
        Box<Service<Request = Message<&'static str, MockBodyStream>,
                    Response = Message<&'static str, Body<u32, io::Error>>,
                    Error = io::Error,
                    Future = Response<Message<&'static str, Body<u32, io::Error>>,
                                              io::Error>>>,
        Box<Any>)
{
    drop(env_logger::init());

    let (ctl, proto) = transport();

    let (tx, rx) = oneshot::channel();
    let (finished_tx, finished_rx) = oneshot::channel();
    let t = thread::spawn(move || {
        let mut core = Core::new().unwrap();
        let handle = core.handle();

        let service = proto.bind_client(&handle, MockIo);
        tx.complete(service);
        drop(core.run(finished_rx));
    });

    let service = rx.wait().unwrap();

    let srv = CompleteOnDrop {
        thread: Some(t),
        tx: Some(finished_tx),
    };
    return (ctl, Box::new(service), Box::new(srv));
}

pub fn multiplex_server<S>(s: S)
    -> (MockTransportCtl<multiplex::Frame<&'static str, u32, io::Error>>, Box<Any>)
    where S: Service<Request = Message<&'static str, Body<u32, io::Error>>,
                     Response = Message<&'static str, MockBodyStream>,
                     Error = io::Error> + Send + 'static,
{
    drop(env_logger::init());

    let (ctl, proto) = transport();

    let (finished_tx, finished_rx) = oneshot::channel();
    let t = thread::spawn(move || {
        let mut core = Core::new().unwrap();
        let handle = core.handle();

        proto.bind_server(&handle, MockIo, s);
        drop(core.run(finished_rx));
    });

    let srv = CompleteOnDrop {
        thread: Some(t),
        tx: Some(finished_tx),
    };
    return (ctl, Box::new(srv));
}
