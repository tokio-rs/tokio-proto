extern crate futures;
extern crate tokio_core;
extern crate tokio_proto;
extern crate tokio_service;
extern crate rand;

#[macro_use]
extern crate log;
extern crate env_logger;

mod support;

use futures::stream::{Receiver};
use futures::{Future, finished, oneshot};
use support::mock;
use tokio_proto::multiplex::{self, RequestId, Frame, Message};
use tokio_core::reactor::Core;
use rand::Rng;
use std::io;
use std::sync::{mpsc, Arc, Mutex};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;

// The message type is a static string for both the request and response
type Msg = &'static str;

// The body stream is a stream of u32 values
type Body = Receiver<u32, io::Error>;

// Frame written to the transport
type InFrame = Frame<Msg, u32, io::Error>;
type OutFrame = Frame<multiplex::Message<Msg, Body>, u32, io::Error>;

#[test]
fn test_immediate_done() {
    let service = tokio_service::simple_service(|req| {
        finished(req)
    });

    run(service, |mock| {
        mock.send(multiplex::Frame::Done);
        mock.allow_and_assert_drop();
    });
}

#[test]
fn test_immediate_writable_echo() {
    let service = tokio_service::simple_service(|req| {
        assert_eq!(req, "hello");
        finished((req))
    });

    run(service, |mock| {
        mock.allow_write();
        mock.send(msg(0, "hello"));

        let wr = mock.next_write();
        assert_eq!(wr.request_id(), Some(0));
        assert_eq!(wr.unwrap_msg(), "hello");

        mock.send(Frame::Done);
        mock.allow_and_assert_drop();
    });
}

#[test]
fn test_immediate_writable_delayed_response_echo() {
    let (c, fut) = oneshot();
    let fut = Mutex::new(Some(fut));

    let service = tokio_service::simple_service(move |req| {
        assert_eq!(req, "hello");
        fut.lock().unwrap().take().unwrap().then(|r| r.unwrap())
    });

    run(service, |mock| {
        mock.allow_write();
        mock.send(msg(0, "hello"));

        support::sleep_ms(20);
        c.complete(Ok(Message::WithoutBody("goodbye")));

        let wr = mock.next_write();
        assert_eq!(wr.request_id(), Some(0));
        assert_eq!(wr.unwrap_msg(), "goodbye");

        mock.send(Frame::Done);
        mock.allow_and_assert_drop();
    });
}

#[test]
fn test_delayed_writable_immediate_response_echo() {
    let service = tokio_service::simple_service(|req| {
        assert_eq!(req, "hello");
        finished((req))
    });

    run(service, |mock| {
        mock.send(msg(0, "hello"));

        support::sleep_ms(20);

        mock.allow_write();

        let wr = mock.next_write();
        assert_eq!(wr.request_id(), Some(0));
        assert_eq!(wr.unwrap_msg(), "hello");
    });
}

#[test]
fn test_same_order_multiplexing() {
    let (tx, rx) = channel();

    let service = tokio_service::simple_service(move |_| {
        let (c, fut) = oneshot();
        tx.lock().unwrap().send(c).unwrap();
        fut.then(|r| r.unwrap())
    });

    run(service, |mock| {
        // Allow all the writes
        for _ in 0..3 { mock.allow_write() };

        mock.send(msg(0, "hello"));
        let c1 = rx.recv().unwrap();

        mock.send(msg(1, "hello"));
        let c2 = rx.recv().unwrap();

        mock.send(msg(2, "hello"));
        let c3 = rx.recv().unwrap();

        mock.assert_no_write(20);
        c1.complete(Ok(Message::WithoutBody("one")));
        c2.complete(Ok(Message::WithoutBody("two")));
        c3.complete(Ok(Message::WithoutBody("three")));

        let wr = mock.next_write();
        assert_eq!(Some(0), wr.request_id());
        assert_eq!("one", wr.unwrap_msg());

        let wr = mock.next_write();
        assert_eq!(Some(1), wr.request_id());
        assert_eq!("two", wr.unwrap_msg());

        let wr = mock.next_write();
        assert_eq!(Some(2), wr.request_id());
        assert_eq!("three", wr.unwrap_msg());
    });
}

#[test]
fn test_out_of_order_multiplexing() {
    let (tx, rx) = channel();

    let service = tokio_service::simple_service(move |_| {
        let (c, fut) = oneshot();
        tx.lock().unwrap().send(c).unwrap();
        fut.then(|r| r.unwrap())
    });

    run(service, |mock| {
        // Allow all the writes
        for _ in 0..3 { mock.allow_write() };

        mock.send(msg(0, "hello"));
        let c1 = rx.recv().unwrap();

        mock.send(msg(1, "hello"));
        let c2 = rx.recv().unwrap();

        mock.send(msg(2, "hello"));
        let c3 = rx.recv().unwrap();

        mock.assert_no_write(20);
        c3.complete(Ok(Message::WithoutBody("three")));

        let wr = mock.next_write();
        assert_eq!(Some(2), wr.request_id());
        assert_eq!("three", wr.unwrap_msg());

        c2.complete(Ok(Message::WithoutBody("two")));

        let wr = mock.next_write();
        assert_eq!(Some(1), wr.request_id());
        assert_eq!("two", wr.unwrap_msg());


        c1.complete(Ok(Message::WithoutBody("one")));

        let wr = mock.next_write();
        assert_eq!(Some(0), wr.request_id());
        assert_eq!("one", wr.unwrap_msg());
    });
}

#[test]
fn test_multiplexing_while_transport_not_writable() {
    let (tx, rx) = channel();

    let service = tokio_service::simple_service(move |req: Message<&'static str, Body>| {
        tx.lock().unwrap().send(req.clone()).unwrap();
        finished(req)
    });

    run(service, |mock| {
        mock.send(msg(0, "one"));
        mock.send(msg(1, "two"));
        mock.send(msg(2, "three"));

        // Assert the service received all the requests before they are written
        // to the transport
        assert_eq!("one", rx.recv().unwrap());
        assert_eq!("two", rx.recv().unwrap());
        assert_eq!("three", rx.recv().unwrap());

        mock.allow_write();
        assert_eq!("one", mock.next_write().unwrap_msg());

        mock.allow_write();
        assert_eq!("two", mock.next_write().unwrap_msg());

        mock.allow_write();
        assert_eq!("three", mock.next_write().unwrap_msg());
    });
}


#[test]
fn test_repeatedly_flushes_messages() {
    let service = tokio_service::simple_service(move |req| {
        finished(req)
    });

    run(service, |mock| {
        mock.send(msg(0, "hello"));

        mock.allow_and_assert_flush();
        mock.allow_and_assert_flush();

        mock.allow_write();
        assert_eq!("hello", mock.next_write().unwrap_msg());

        mock.send(Frame::Done);
        mock.assert_drop();
    });
}

#[test]
fn test_reaching_max_in_flight_requests() {
    use futures::Oneshot;

    let (tx, rx) = mpsc::channel::<Oneshot<Result<Message<&'static str, Body>, io::Error>>>();
    let rx = Arc::new(Mutex::new(rx));

    let c1 = Arc::new(AtomicUsize::new(0));
    let c2 = c1.clone();

    let service = tokio_service::simple_service(move |_| {
        c2.fetch_add(1, Ordering::Relaxed);
        let fut = rx.lock().unwrap().recv().unwrap();
        fut.map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "broken pipe"))
           .and_then(|res| res)
        // fut.then(|res| res.or_else(|_| Err(io::Error::new(io::ErrorKind::BrokenPipe, "broken pipe"))))
    });

    let mut responses = vec![];

    run(service, |mock| {
        for i in 0..33 {
            let (c, resp) = oneshot();
            tx.send(resp).unwrap();
            responses.push((i, c));
            mock.send(msg(i, "request"));
        }

        println!("assert no writes");

        // wait a bit
        mock.assert_no_write(100);

        println!("check cnt");

        // Only 32 requests processed
        assert_eq!(32, c1.load(Ordering::Relaxed));

        println!("shuffle");

        // Pick a random request to complete
        rand::thread_rng().shuffle(&mut responses);
        let (i, c) = responses.remove(0);

        println!("complete");
        c.complete(Ok(Message::WithoutBody("zomg")));

        mock.assert_no_write(50);

        // Next request not yet processed
        assert_eq!(32, c1.load(Ordering::Relaxed));

        // allow the write
        mock.allow_write();

        println!("get write");
        // Read the response
        let wr = mock.next_write();
        assert_eq!(Some(i), wr.request_id());
        assert_eq!("zomg", wr.unwrap_msg());

        println!("YAAAAH");
        mock.assert_no_write(50);

        // Next request is processed
        assert_eq!(33, c1.load(Ordering::Relaxed));
    });
}

#[test]
fn test_reaching_max_buffered_frames() {
}

fn channel<T>() -> (Arc<Mutex<mpsc::Sender<T>>>, mpsc::Receiver<T>) {
    let (tx, rx) = mpsc::channel();
    let tx = Arc::new(Mutex::new(tx));
    (tx, rx)
}

fn msg(request_id: RequestId, msg: Msg) -> OutFrame {
    Frame::Message(request_id, Message::WithoutBody(msg))
}

/// Setup a reactor running a multiplex::Server with the given service and a
/// mock transport. Yields the mock transport handle to the function.
fn run<S, F>(service: S, f: F)
    where S: multiplex::ServerService<Request = multiplex::Message<Msg, Body>,
                                     Response = Msg,
                                         Body = u32,
                                   BodyStream = Body,
                                        Error = io::Error> + Send + 'static,
          S::Future: Send + 'static,
          F: FnOnce(mock::TransportHandle<InFrame, OutFrame>),
{
    drop(::env_logger::init());
    let (tx, rx) = oneshot();
    let (tx2, rx2) = mpsc::channel();
    let t = thread::spawn(move || {
        let mut lp = Core::new().unwrap();
        let handle = lp.handle();
        let (mock, new_transport) = mock::transport::<InFrame, OutFrame>(handle.clone());

        let transport = new_transport.new_transport().unwrap();
        handle.spawn({
            let dispatch = multiplex::Server::new(service, transport).unwrap();
            dispatch.map_err(|e| error!("error: {}", e))
        });
        tx2.send(mock).unwrap();
        lp.run(rx)
    });
    let mock = rx2.recv().unwrap();

    f(mock);

    tx.complete(());
    t.join().unwrap().unwrap();
}
