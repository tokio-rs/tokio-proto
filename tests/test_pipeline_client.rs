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
use std::thread;
use std::time::Duration;

use futures::sync::mpsc;
use futures::{Future, Stream, Sink};
use tokio_proto::streaming::Message;
use tokio_proto::streaming::pipeline::Frame;
use tokio_service::Service;

mod support;
use support::mock;

#[test]
fn test_ping_pong_close() {
    let (mut mock, service, _other) = mock::pipeline_client();

    let pong = service.call(Message::WithoutBody("ping"));
    assert_eq!("ping", mock.next_write().unwrap_msg());
    mock.send(msg("pong"));
    assert_eq!("pong", pong.wait().unwrap().into_inner());
    mock.allow_and_assert_drop();
}


#[test]
#[ignore]
fn test_response_ready_before_request_sent() {
    let (mut mock, service, _other) = mock::pipeline_client();

    mock.send(msg("pong"));

    thread::sleep(Duration::from_millis(20));

    let pong = service.call(Message::WithoutBody("ping"));

    assert_eq!("pong", pong.wait().unwrap().into_inner());
}

#[test]
fn test_streaming_request_body() {
    let (mut mock, service, _other) = mock::pipeline_client();

    let (mut tx, rx) = mpsc::channel(1);

    let pong = service.call(Message::WithBody("ping",
                                              rx.then(|r| r.unwrap()).boxed()));

    assert_eq!("ping", mock.next_write().unwrap_msg());

    for i in 0..3 {
        tx = tx.send(Ok(i)).wait().unwrap();
        assert_eq!(Some(i), mock.next_write().unwrap_body());
    }

    drop(tx);
    assert_eq!(None, mock.next_write().unwrap_body());

    mock.send(msg("pong"));
    assert_eq!("pong", pong.wait().unwrap().into_inner());

    mock.allow_and_assert_drop();
}

#[test]
#[ignore]
fn test_streaming_response_body() {
}

#[test]
fn test_streaming_client_dropped() {
    let (mut tx, mut mock, pong, _other) = {
        let (mock, service, _other) = mock::pipeline_client();

        let (tx, rx) = mpsc::channel(1);

        let pong = service.call(Message::WithBody("ping",
                                                  rx.then(|r| r.unwrap()).boxed()));
        (tx, mock, pong, _other)
    };

    assert_eq!("ping", mock.next_write().unwrap_msg());

    for i in 0..3 {
        tx = tx.send(Ok(i)).wait().unwrap();
        assert_eq!(Some(i), mock.next_write().unwrap_body());
    }

    drop(tx);
    assert_eq!(None, mock.next_write().unwrap_body());

    mock.send(msg("pong"));
    assert_eq!("pong", pong.wait().unwrap().into_inner());

    mock.allow_and_assert_drop();
}

#[test]
fn test_streaming_client_transport_dropped() {
    let (mut mock, service, _) = mock::pipeline_client();
    let pong = service.call(Message::WithoutBody("ping"));

    assert_eq!(pong.wait().unwrap_err().kind(), io::ErrorKind::BrokenPipe);

    mock.allow_and_assert_drop();
}

fn msg(msg: &'static str) -> Frame<&'static str, u32, io::Error> {
    Frame::Message {
        message: msg,
        body: false,
    }
}
