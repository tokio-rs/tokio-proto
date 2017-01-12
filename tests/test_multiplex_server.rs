#![allow(deprecated)]

extern crate futures;
extern crate tokio_core;
extern crate tokio_proto;
extern crate tokio_service;
extern crate rand;

#[macro_use]
extern crate log;

use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::cell::RefCell;
use std::thread;
use std::time::Duration;

use futures::{Future, Stream, Sink};
use futures::future;
use futures::sync::oneshot;
use futures::sync::mpsc;
use tokio_proto::streaming::{Message, Body};
use tokio_proto::streaming::multiplex::{Frame, RequestId};
use rand::Rng;

mod support;
use support::mock;
use support::service::simple_service;

#[test]
fn test_immediate_done() {
    let service = simple_service(|_| {
        future::ok(Message::WithoutBody("goodbye"))
    });

    let (mut mock, _other) = mock::multiplex_server(service);
    mock.allow_and_assert_drop();
}

#[test]
fn test_immediate_writable_echo() {
    let service = simple_service(|req| {
        assert_eq!(req, "hello");
        future::ok(Message::WithoutBody("goodbye"))
    });

    let (mut mock, _other) = mock::multiplex_server(service);
    mock.send(msg(0, "hello"));

    let wr = mock.next_write();
    assert_eq!(wr.request_id(), 0);
    assert_eq!(wr.unwrap_msg(), "goodbye");

    mock.allow_and_assert_drop();
}

#[test]
fn test_immediate_writable_delayed_response_echo() {
    let (c, fut) = oneshot::channel();
    let fut = RefCell::new(Some(fut));

    let service = simple_service(move |req| {
        assert_eq!(req, "hello");
        fut.borrow_mut().take().unwrap().then(|r| r.unwrap())
    });

    let (mut mock, _other) = mock::multiplex_server(service);
    mock.send(msg(0, "hello"));

    thread::sleep(Duration::from_millis(20));
    c.complete(Ok(Message::WithoutBody("goodbye")));

    let wr = mock.next_write();
    assert_eq!(wr.request_id(), 0);
    assert_eq!(wr.unwrap_msg(), "goodbye");

    mock.allow_and_assert_drop();
}

#[test]
fn test_delayed_writable_immediate_response_echo() {
    let service = simple_service(|req| {
        assert_eq!(req, "hello");
        future::ok(Message::WithoutBody("goodbye"))
    });

    let (mut mock, _other) = mock::multiplex_server(service);
    mock.send(msg(0, "hello"));

    thread::sleep(Duration::from_millis(20));

    let wr = mock.next_write();
    assert_eq!(wr.request_id(), 0);
    assert_eq!(wr.unwrap_msg(), "goodbye");
}

#[test]
fn test_same_order_multiplexing() {
    let (tx, rx) = mpsc::unbounded();

    let service = simple_service(move |_| {
        let (c, fut) = oneshot::channel();
        mpsc::UnboundedSender::send(&mut tx.clone(), c).unwrap();
        fut.then(|r| r.unwrap())
    });

    let (mut mock, _other) = mock::multiplex_server(service);

    let mut rx = rx.wait();
    mock.send(msg(0, "hello"));
    let c1 = rx.next().unwrap().unwrap();

    mock.send(msg(1, "hello"));
    let c2 = rx.next().unwrap().unwrap();

    mock.send(msg(2, "hello"));
    let c3 = rx.next().unwrap().unwrap();

    c1.complete(Ok(Message::WithoutBody("one")));
    thread::sleep(Duration::from_millis(20));

    c2.complete(Ok(Message::WithoutBody("two")));
    thread::sleep(Duration::from_millis(20));

    c3.complete(Ok(Message::WithoutBody("three")));

    let wr = mock.next_write();
    assert_eq!(0, wr.request_id());
    assert_eq!("one", wr.unwrap_msg());

    let wr = mock.next_write();
    assert_eq!(1, wr.request_id());
    assert_eq!("two", wr.unwrap_msg());

    let wr = mock.next_write();
    assert_eq!(2, wr.request_id());
    assert_eq!("three", wr.unwrap_msg());
}

#[test]
fn test_out_of_order_multiplexing() {
    let (tx, rx) = mpsc::unbounded();

    let service = simple_service(move |_| {
        let (c, fut) = oneshot::channel();
        mpsc::UnboundedSender::send(&mut tx.clone(), c).unwrap();
        fut.then(|r| r.unwrap())
    });

    let (mut mock, _other) = mock::multiplex_server(service);

    let mut rx = rx.wait();
    mock.send(msg(0, "hello"));
    let c1 = rx.next().unwrap().unwrap();

    mock.send(msg(1, "hello"));
    let c2 = rx.next().unwrap().unwrap();

    mock.send(msg(2, "hello"));
    let c3 = rx.next().unwrap().unwrap();

    c3.complete(Ok(Message::WithoutBody("three")));

    let wr = mock.next_write();
    assert_eq!(2, wr.request_id());
    assert_eq!("three", wr.unwrap_msg());

    c2.complete(Ok(Message::WithoutBody("two")));

    let wr = mock.next_write();
    assert_eq!(1, wr.request_id());
    assert_eq!("two", wr.unwrap_msg());


    c1.complete(Ok(Message::WithoutBody("one")));

    let wr = mock.next_write();
    assert_eq!(0, wr.request_id());
    assert_eq!("one", wr.unwrap_msg());
}

#[test]
fn test_multiplexing_while_transport_not_writable() {
    let (tx, rx) = mpsc::unbounded();

    let service = simple_service(move |req: Message<&'static str, Body<u32, io::Error>>| {
        mpsc::UnboundedSender::send(&mut tx.clone(), req.clone()).unwrap();
        future::ok(Message::WithoutBody(*req.get_ref()))
    });

    let (mut mock, _other) = mock::multiplex_server(service);
    mock.send(msg(0, "one"));
    mock.send(msg(1, "two"));
    mock.send(msg(2, "three"));

    // Assert the service received all the requests before they are written
    // to the transport
    let mut rx = rx.wait();
    assert_eq!("one", rx.next().unwrap().unwrap());
    assert_eq!("two", rx.next().unwrap().unwrap());
    assert_eq!("three", rx.next().unwrap().unwrap());

    assert_eq!("one", mock.next_write().unwrap_msg());
    assert_eq!("two", mock.next_write().unwrap_msg());
    assert_eq!("three", mock.next_write().unwrap_msg());
}

#[test]
fn test_repeatedly_flushes_messages() {
    let service = simple_service(move |_| {
        future::ok(Message::WithoutBody("goodbye"))
    });

    let (mut mock, _other) = mock::multiplex_server(service);
    mock.send(msg(0, "hello"));

    assert_eq!("goodbye", mock.next_write().unwrap_msg());

    mock.allow_and_assert_drop();
}

#[test]
fn test_reaching_max_in_flight_requests() {
    let (mut tx, rx) = mpsc::unbounded();
    let rx = RefCell::new(rx.wait());

    let c1 = Arc::new(AtomicUsize::new(0));
    let c2 = c1.clone();

    let service = simple_service(move |_| {
        c2.fetch_add(1, Ordering::SeqCst);
        let fut = rx.borrow_mut().next().unwrap().unwrap();
        let fut: oneshot::Receiver<_> = fut;
        fut.map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "broken pipe"))
           .and_then(|res| res)
    });

    let mut responses = vec![];

    let (mut mock, _other) = mock::multiplex_server(service);
    for i in 0..33 {
        let (c, resp) = oneshot::channel();
        mpsc::UnboundedSender::send(&mut tx, resp).unwrap();
        responses.push((i, c));
        mock.send(msg(i, "request"));
    }

    // Only 32 requests processed
    while c1.load(Ordering::SeqCst) < 32 {
        thread::yield_now();
    }

    // Pick one from the first 32 requests to complete.
    rand::thread_rng().shuffle(&mut responses[0..32]);
    let (i, c) = responses.remove(0);

    c.complete(Ok(Message::WithoutBody("zomg")));

    // Next request not yet processed
    assert_eq!(32, c1.load(Ordering::SeqCst));

    // Read the response
    let wr = mock.next_write();
    assert_eq!(i, wr.request_id());
    assert_eq!("zomg", wr.unwrap_msg());

    // Next request is processed
    while 33 != c1.load(Ordering::SeqCst) {
        thread::yield_now();
    }

    // Complete pending requests
    for (i, c) in responses.drain(..) {
        c.complete((Ok(Message::WithoutBody("zomg"))));

        let wr = mock.next_write();
        assert_eq!(i, wr.request_id());
        assert_eq!("zomg", wr.unwrap_msg());
    }

    mock.allow_and_assert_drop();
}

#[test]
fn test_basic_streaming_response_body() {
    let (tx, rx) = mpsc::channel(1);
    let rx = RefCell::new(Some(rx));

    let service = simple_service(move |req| {
        assert_eq!(req, "want-body");

        let rx = rx.borrow_mut().take().unwrap();
        let rx = rx.then(|r| r.unwrap());
        future::ok(Message::WithBody("hi2u", rx.boxed()))
    });

    let (mut mock, _other) = mock::multiplex_server(service);
    mock.send(msg(3, "want-body"));

    let wr = mock.next_write();
    assert_eq!(3, wr.request_id());
    assert_eq!(wr.unwrap_msg(), "hi2u");

    // Allow the write, then send the message
    let tx = tx.send(Ok(1)).wait().unwrap();
    let wr = mock.next_write();
    assert_eq!(3, wr.request_id());
    assert_eq!(Some(1), wr.unwrap_body());

    // Send the message then allow the write
    let tx = tx.send(Ok(2)).wait().ok().unwrap();
    let wr = mock.next_write();
    assert_eq!(3, wr.request_id());
    assert_eq!(Some(2), wr.unwrap_body());

    drop(tx);

    let wr = mock.next_write();
    assert_eq!(3, wr.request_id());
    assert_eq!(None, wr.unwrap_body());

    // Alright, clean shutdown
    mock.allow_and_assert_drop();
}

#[test]
fn test_basic_streaming_request_body_read_then_respond() {
    let (tx, rx) = mpsc::unbounded();

    let service = simple_service(move |mut req: Message<&'static str, Body<u32, io::Error>>| {
        assert_eq!(req, "have-body");

        let body = req.take_body().unwrap();
        let mut tx = tx.clone();

        body.for_each(move |chunk| {
            mpsc::UnboundedSender::send(&mut tx, chunk).unwrap();
            Ok(())
        }).and_then(|_| {
            Ok(Message::WithoutBody("hi2u"))
        })
    });

    let (mut mock, _other) = mock::multiplex_server(service);
    mock.send(msg_with_body(2, "have-body"));

    let mut rx = rx.wait();
    for i in 0..5 {
        // Send a body chunk
        mock.send(Frame::Body { id: 2, chunk: Some(i) });

        // Assert service processed chunk
        assert_eq!(i, rx.next().unwrap().unwrap());
    }

    // Send end-of-stream notification
    mock.send(Frame::Body { id: 2, chunk: None });

    let wr = mock.next_write();
    assert_eq!(2, wr.request_id());
    assert_eq!("hi2u", wr.unwrap_msg());

    // Clean shutdown
    mock.allow_and_assert_drop();
}

#[test]
fn test_interleaving_request_body_chunks() {
    let (tx, rx) = mpsc::unbounded();
    let cnt = AtomicUsize::new(0);

    let service = simple_service(move |mut req: Message<&'static str, Body<u32, io::Error>>| {
        let body = req.take_body().unwrap();
        let mut tx = tx.clone();
        let i = cnt.fetch_add(1, Ordering::Relaxed);

        assert_eq!(req, &format!("have-body-{}", i));

        body.for_each(move |chunk| {
            mpsc::UnboundedSender::send(&mut tx, (i, chunk)).unwrap();
            Ok(())
        }).and_then(|_| {
            Ok(Message::WithoutBody("hi2u"))
        })
    });

    let (mut mock, _other) = mock::multiplex_server(service);
    mock.send(msg_with_body(2, "have-body-0"));
    mock.send(msg_with_body(4, "have-body-1"));

    let mut rx = rx.wait();
    for i in 0..5 {
        if i % 2 == 0 {
            // Send a body chunk
            mock.send(Frame::Body { id: 2, chunk: Some(i) });
            assert_eq!((0, i), rx.next().unwrap().unwrap());

            mock.send(Frame::Body { id: 4, chunk: Some(i) });
            assert_eq!((1, i), rx.next().unwrap().unwrap());
        } else {
            mock.send(Frame::Body { id: 4, chunk: Some(i) });
            assert_eq!((1, i), rx.next().unwrap().unwrap());

            // Send a body chunk
            mock.send(Frame::Body { id: 2, chunk: Some(i) });
            assert_eq!((0, i), rx.next().unwrap().unwrap());
        }
    }

    // Send end-of-stream notification
    mock.send(Frame::Body { id: 2, chunk: None });

    let wr = mock.next_write();
    assert_eq!(2, wr.request_id());
    assert_eq!("hi2u", wr.unwrap_msg());

    mock.send(Frame::Body { id: 4, chunk: None });

    let wr = mock.next_write();
    assert_eq!(4, wr.request_id());
    assert_eq!("hi2u", wr.unwrap_msg());

    // Clean shutdown
    mock.allow_and_assert_drop();
}

#[test]
#[ignore]
fn test_interleaving_response_body_chunks() {
}

#[test]
#[ignore]
fn test_transport_provides_invalid_request_ids() {
}

#[test]
#[ignore]
fn test_reaching_max_buffered_frames() {
}

#[test]
fn test_read_error_as_first_frame() {
    let service = simple_service(|_| {
        // Makes the compiler happy
        if true {
            panic!("should not be called");
        }

        future::ok(Message::WithoutBody("nope"))
    });

    let (mut mock, _other) = mock::multiplex_server(service);
    mock.send(Frame::Error {
        id: 1,
        error: io::Error::new(io::ErrorKind::Other, "boom"),
    });
    mock.allow_and_assert_drop();
}

#[test]
#[ignore]
fn test_read_error_during_stream() {
}

#[test]
#[ignore]
fn test_error_handling_before_message_dispatched() {
}

fn msg(id: RequestId, msg: &'static str) -> Frame<&'static str, u32, io::Error> {
    Frame::Message {
        id: id,
        message: msg,
        body: false,
        solo: false,
    }
}

fn msg_with_body(id: RequestId, msg: &'static str) -> Frame<&'static str, u32, io::Error> {
    Frame::Message {
        id: id,
        message: msg,
        body: true,
        solo: false,
    }
}
