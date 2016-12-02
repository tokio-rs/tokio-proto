#![allow(deprecated)]

extern crate futures;
extern crate tokio_core;
extern crate tokio_proto;
extern crate tokio_service;
extern crate rand;

#[macro_use]
extern crate log;
extern crate env_logger;

use std::io;

use futures::stream::{Stream};
use futures::{Future};
use tokio_proto::streaming::Message;
use tokio_proto::streaming::multiplex::{RequestId, Frame};
use tokio_service::Service;

mod support;
use support::mock;

#[test]
fn test_ping_pong_close() {
    let (mut mock, service, _other) = mock::multiplex_client();

    let pong = service.call(Message::WithoutBody("ping"));
    let wr = mock.next_write();
    assert_eq!(0, wr.request_id());
    assert_eq!("ping", wr.unwrap_msg());

    mock.send(msg(0, "pong"));
    assert_eq!("pong", pong.wait().unwrap().into_inner());

    mock.allow_and_assert_drop();
}

#[test]
fn test_error_on_response() {
    let (mut mock, service, _other) = mock::multiplex_client();

    let pong = service.call(Message::WithoutBody("ping"));

    let wr = mock.next_write();
    assert_eq!(0, wr.request_id());
    assert_eq!("ping", wr.unwrap_msg());

    mock.send(Frame::Error {
        id: 0,
        error: io::Error::new(io::ErrorKind::Other, "nope"),
    });

    assert_eq!(io::ErrorKind::Other, pong.wait().unwrap_err().kind());

    mock.allow_and_assert_drop();
}

#[test]
fn drop_client_while_streaming_body() {
    let (mut mock, service, _other) = mock::multiplex_client();

    let pong = service.call(Message::WithoutBody("ping"));

    let wr = mock.next_write();
    assert_eq!(0, wr.request_id());
    assert_eq!("ping", wr.unwrap_msg());

    mock.send(msg_with_body(0, "pong"));

    let mut pong = pong.wait().unwrap();
    assert_eq!("pong", &pong.get_ref()[..]);

    let rx = pong.take_body().unwrap();

    // Drop the client
    drop(service);

    // Send body frames
    for i in 0..3 {
        mock.send(body(0, Some(i)));
    }

    mock.send(body(0, None));

    let body: Vec<u32> = rx.wait().map(|i| i.unwrap()).collect();
    assert_eq!(&[0, 1, 2], &body[..]);

    mock.allow_and_assert_drop();
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

fn body(id: RequestId, body: Option<u32>) -> Frame<&'static str, u32, io::Error> {
    Frame::Body {
        id: id,
        chunk: body,
    }
}
