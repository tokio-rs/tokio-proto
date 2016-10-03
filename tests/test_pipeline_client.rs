extern crate futures;
extern crate tokio_core;
extern crate tokio_proto;
extern crate tokio_service;
extern crate rand;

#[macro_use]
extern crate log;
extern crate env_logger;

mod support;

use futures::stream::{self, Receiver};
use futures::{Future, oneshot};
use support::mock;
use tokio_service::Service;
use tokio_proto::{pipeline, Message};
use tokio_core::reactor::Core;
use std::io;
use std::thread;
use std::sync::mpsc;

// The message type is a static string for both the request and response
type Msg = &'static str;

// Transport handle
type TransportHandle = mock::TransportHandle<Frame, Frame>;

// Client handle
type Client = tokio_proto::Client<Msg, Msg, Body, Body, io::Error>;

// In frame
type Frame = pipeline::Frame<Msg, u32, io::Error>;

// Body stream
type Body = Receiver<u32, io::Error>;

#[test]
fn test_ping_pong_close() {
    run(|mock, service| {
        mock.allow_write();

        let pong = service.call(Message::WithoutBody("ping"));
        assert_eq!("ping", mock.next_write().unwrap_msg());

        mock.send(msg("pong"));
        assert_eq!("pong", pong.wait().unwrap().into_inner());

        mock.send(pipeline::Frame::Done);
        mock.allow_and_assert_drop();
    });
}

#[test]
#[ignore]
fn test_response_ready_before_request_sent() {
    run(|mock, service| {
        mock.send(msg("pong"));

        support::sleep_ms(20);

        let pong = service.call(Message::WithoutBody("ping"));

        assert_eq!("pong", pong.wait().unwrap().into_inner());
    });
}

#[test]
fn test_streaming_request_body() {
    run(|mock, service| {
        let (mut tx, rx) = stream::channel();

        mock.allow_write();
        let pong = service.call(Message::WithBody("ping", rx));

        assert_eq!("ping", mock.next_write().unwrap_msg());

        for i in 0..3 {
            mock.allow_write();
            tx = tx.send(Ok(i)).wait().ok().unwrap();
            assert_eq!(Some(i), mock.next_write().unwrap_body());
        }

        mock.allow_write();
        drop(tx);
        assert_eq!(None, mock.next_write().unwrap_body());

        mock.send(msg("pong"));
        assert_eq!("pong", pong.wait().unwrap().into_inner());

        mock.send(pipeline::Frame::Done);
        mock.allow_and_assert_drop();
    });
}

#[test]
#[ignore]
fn test_streaming_response_body() {
}

fn msg(msg: Msg) -> Frame {
    pipeline::Frame::Message {
        message: msg,
        body: false,
    }
}

/// Setup a reactor running a pipeline::Client and a mock transport. Yields the
/// mock transport handle to the function.
fn run<F>(f: F) where F: FnOnce(TransportHandle, Client) {
    let _ = ::env_logger::init();

    let (tx, rx) = oneshot();
    let (tx2, rx2) = mpsc::channel();
    let t = thread::spawn(move || {
        let mut lp = Core::new().unwrap();
        let handle = lp.handle();
        let (mock, new_transport) = mock::transport::<Frame, Frame>(handle.clone());

        let transport = new_transport.new_transport().unwrap();
        let service = pipeline::connect(Ok(transport), &handle);

        tx2.send((mock, service)).unwrap();
        lp.run(rx)
    });

    let (mock, service) = rx2.recv().unwrap();

    f(mock, service);

    tx.complete(());
    t.join().unwrap().unwrap();
}
