#![allow(deprecated)]

extern crate futures;
extern crate tokio_core;
extern crate tokio_proto;
extern crate tokio_service;
extern crate rand;

#[macro_use]
extern crate log;
extern crate env_logger;

use std::cell::RefCell;
use std::io;
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use futures::stream;
use futures::sync::mpsc;
use futures::sync::oneshot;
use futures::future;
use futures::{Future, Stream, Sink};
use tokio_proto::streaming::pipeline::Frame;
use tokio_proto::streaming::{Message, Body};

mod support;
use support::service::simple_service;
use support::mock;

#[test]
fn test_immediate_done() {
    let service = simple_service(|_| {
        future::ok(Message::WithoutBody("goodbye"))
    });

    let (mut mock, _other) = mock::pipeline_server(service);
    mock.allow_and_assert_drop();
}

#[test]
fn test_immediate_writable_echo() {
    let service = simple_service(|req: Message<&'static str, Body<u32, io::Error>>| {
        assert_eq!(req, "hello");
        future::finished(Message::WithoutBody(*req.get_ref()))
    });

    let (mut mock, _other) = mock::pipeline_server(service);
    mock.send(msg("hello"));
    assert_eq!(mock.next_write().unwrap_msg(), "hello");
    mock.allow_and_assert_drop();
}

#[test]
fn test_immediate_writable_delayed_response_echo() {
    let (c, fut) = oneshot::channel();
    let fut = Mutex::new(Some(fut));

    let service = simple_service(move |req| {
        assert_eq!(req, "hello");
        fut.lock().unwrap().take().unwrap().then(|r| r.unwrap())
    });

    let (mut mock, _other) = mock::pipeline_server(service);
    mock.send(msg("hello"));

    thread::sleep(Duration::from_millis(20));
    c.complete(Ok(Message::WithoutBody("goodbye")));

    assert_eq!(mock.next_write().unwrap_msg(), "goodbye");

    mock.allow_and_assert_drop();
}

#[test]
fn test_delayed_writable_immediate_response_echo() {
    let service = simple_service(|req: Message<&'static str, Body<u32, io::Error>>| {
        assert_eq!(req, "hello");
        future::finished(Message::WithoutBody(*req.get_ref()))
    });

    let (mut mock, _other) = mock::pipeline_server(service);
    mock.send(msg("hello"));

    thread::sleep(Duration::from_millis(20));

    assert_eq!(mock.next_write().unwrap_msg(), "hello");
}

#[test]
fn test_pipelining_while_service_is_processing() {
    let (tx, rx) = mpsc::unbounded();
    let tx = RefCell::new(tx);

    let service = simple_service(move |_| {
        let (c, fut) = oneshot::channel();
        mpsc::UnboundedSender::send(&mut tx.borrow_mut(), c).unwrap();
        fut.then(|r| r.unwrap())
    });

    let (mut mock, _other) = mock::pipeline_server(service);
    let mut rx = rx.wait();

    mock.send(msg("hello"));
    let c1 = rx.next().unwrap().unwrap();

    mock.send(msg("hello"));
    let c2 = rx.next().unwrap().unwrap();

    mock.send(msg("hello"));
    let c3 = rx.next().unwrap().unwrap();

    c3.complete(Ok(Message::WithoutBody("three")));
    c2.complete(Ok(Message::WithoutBody("two")));
    c1.complete(Ok(Message::WithoutBody("one")));

    assert_eq!("one", mock.next_write().unwrap_msg());
    assert_eq!("two", mock.next_write().unwrap_msg());
    assert_eq!("three", mock.next_write().unwrap_msg());
}

#[test]
fn test_pipelining_while_transport_not_writable() {
    let (tx, rx) = mpsc::unbounded();
    let tx = RefCell::new(tx);

    let service = simple_service(move |req: Message<&'static str, Body<u32, io::Error>>| {
        mpsc::UnboundedSender::send(&mut tx.borrow_mut(), req.clone()).unwrap();
        future::finished(Message::WithoutBody(*req.get_ref()))
    });

    let (mut mock, _other) = mock::pipeline_server(service);
    mock.send(msg("one"));
    mock.send(msg("two"));
    mock.send(msg("three"));

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
    let service = simple_service(move |req: Message<&'static str, Body<u32, io::Error>>| {
        future::ok(Message::WithoutBody(*req.get_ref()))
    });

    let (mut mock, _other) = mock::pipeline_server(service);
    mock.send(msg("hello"));

    assert_eq!("hello", mock.next_write().unwrap_msg());

    mock.allow_and_assert_drop();
}

#[test]
fn test_returning_error_from_service() {
    let service = simple_service(move |_| {
        future::err(io::Error::new(io::ErrorKind::Other, "nope"))
    });

    let (mut mock, _other) = mock::pipeline_server(service);

    mock.send(msg("hello"));
    assert_eq!(io::ErrorKind::Other, mock.next_write().unwrap_err().kind());
    mock.allow_and_assert_drop();
}

#[test]
fn test_reading_error_frame_from_transport() {
    let service = simple_service(move |_| {
        future::ok(Message::WithoutBody("omg no"))
    });

    let (mut mock, _other) = mock::pipeline_server(service);
    mock.send(Frame::Error {
        error: io::Error::new(io::ErrorKind::Other, "mock transport error frame"),
    });
    mock.allow_and_assert_drop();
}

#[test]
fn test_reading_io_error_from_transport() {
    let service = simple_service(move |_| {
        future::finished(Message::WithoutBody("omg no"))
    });

    let (mut mock, _other) = mock::pipeline_server(service);
    mock.error(io::Error::new(io::ErrorKind::Other, "mock transport error frame"));
    mock.allow_and_assert_drop();
}

#[test]
#[ignore]
fn test_reading_error_while_pipelining_from_transport() {
    unimplemented!();
}

#[test]
#[ignore]
fn test_returning_would_block_from_service() {
    // Because... it could happen
}

#[test]
fn test_streaming_request_body_then_responding() {
    let (tx, rx) = mpsc::unbounded();

    let service = simple_service(move |mut req: Message<&'static str, Body<u32, io::Error>>| {
        assert_eq!(req, "omg");

        let body = req.take_body().unwrap();
        let mut tx = tx.clone();

        body.for_each(move |chunk| {
                mpsc::UnboundedSender::send(&mut tx, chunk).unwrap();
                Ok(())
            })
            .and_then(|_| future::finished(Message::WithoutBody("hi2u")))
    });

    let (mut mock, _other) = mock::pipeline_server(service);
    mock.send(msg_with_body("omg"));

    let mut rx = rx.wait();
    for i in 0..5 {
        mock.send(Frame::Body { chunk: Some(i) });
        assert_eq!(i, rx.next().unwrap().unwrap());
    }

    // Send end-of-stream notification
    mock.send(Frame::Body { chunk: None });

    assert_eq!(mock.next_write().unwrap_msg(), "hi2u");

    mock.allow_and_assert_drop();
}

#[test]
fn test_responding_then_streaming_request_body() {
    let (tx, rx) = mpsc::unbounded();

    let service = simple_service(move |mut req: Message<&'static str, Body<u32, io::Error>>| {
        assert_eq!(req, "omg");

        let body = req.take_body().unwrap();
        let mut tx = tx.clone();

        thread::spawn(|| {
            body.for_each(move |chunk| {
                    mpsc::UnboundedSender::send(&mut tx, chunk).unwrap();
                    Ok(())
                })
                .wait()
                .unwrap();
        });

        future::finished(Message::WithoutBody("hi2u"))
    });

    let (mut mock, _other) = mock::pipeline_server(service);

    mock.send(msg_with_body("omg"));

    assert_eq!(mock.next_write().unwrap_msg(), "hi2u");

    let mut rx = rx.wait();
    for i in 0..5 {
        mock.send(Frame::Body { chunk: Some(i) });
        assert_eq!(i, rx.next().unwrap().unwrap());
    }

    // Send end-of-stream notification
    mock.send(Frame::Body { chunk: None });

    mock.allow_and_assert_drop();
}

#[test]
fn test_pipeline_stream_response_body() {
    let service = simple_service(move |_| {
        let body = stream::once(Ok(1u32)).boxed();
        future::finished(Message::WithBody("resp", body))
    });

    let (mut mock, _other) = mock::pipeline_server(service);

    mock.send(msg("one"));

    assert_eq!(mock.next_write().unwrap_msg(), "resp");
    assert_eq!(mock.next_write().unwrap_body(), Some(1));
    assert_eq!(mock.next_write().unwrap_body(), None);

    mock.allow_and_assert_drop();
}

#[test]
fn test_pipeline_streaming_body_without_consuming() {
    let (tx, rx) = mpsc::unbounded();

    let service = simple_service(move |mut req: Message<&'static str, Body<u32, io::Error>>| {
        let body = req.take_body().unwrap();

        if req == "one" {
            debug!("drop body");
            future::finished(Message::WithoutBody("resp-one")).boxed()
        } else {
            let mut tx = tx.clone();

            body.for_each(move |chunk| {
                    mpsc::UnboundedSender::send(&mut tx, chunk).unwrap();
                    Ok(())
                })
                .and_then(|_| future::finished(Message::WithoutBody("resp-two")))
                .boxed()
        }
    });

    let (mut mock, _other) = mock::pipeline_server(service);
    mock.send(msg_with_body("one"));

    for i in 0..5 {
        mock.send(Frame::Body { chunk: Some(i) });
        thread::sleep(Duration::from_millis(20));
    }

    assert_eq!(mock.next_write().unwrap_msg(), "resp-one");

    // Send the next request
    mock.send(msg_with_body("two"));

    let mut rx = rx.wait();
    for i in 0..5 {
        mock.send(Frame::Body { chunk: Some(i) });
        assert_eq!(i, rx.next().unwrap().unwrap());
    }

    mock.send(Frame::Body { chunk: None });

    assert_eq!(mock.next_write().unwrap_msg(), "resp-two");

    mock.allow_and_assert_drop();
}

#[test]
#[ignore]
fn test_transport_error_during_body_stream() {
}

#[test]
fn test_streaming_response_body() {
    let (tx, rx) = mpsc::channel::<io::Result<u32>>(0);
    let rx = RefCell::new(Some(rx));

    let service = simple_service(move |req| {
        assert_eq!(req, "omg");
        let rx = rx.borrow_mut().take().unwrap();
        let rx = rx.then(|r| r.unwrap()).boxed();
        future::finished(Message::WithBody("hi2u", rx))
    });

    let (mut mock, _other) = mock::pipeline_server(service);
    mock.send(msg("omg"));

    assert_eq!(mock.next_write().unwrap_msg(), "hi2u");

    let tx = tx.send(Ok(1)).wait().unwrap();
    assert_eq!(Some(1), mock.next_write().unwrap_body());

    let _ = tx.send(Ok(2)).wait().unwrap();
    assert_eq!(Some(2), mock.next_write().unwrap_body());

    assert_eq!(None, mock.next_write().unwrap_body());

    mock.allow_and_assert_drop();
}

fn msg(msg: &'static str) -> Frame<&'static str, u32, io::Error> {
    Frame::Message { message: msg, body: false }
}

fn msg_with_body(msg: &'static str) -> Frame<&'static str, u32, io::Error> {
    Frame::Message { message: msg, body: true }
}
