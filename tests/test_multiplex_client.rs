extern crate futures;
extern crate tokio_core;
extern crate tokio_proto;
extern crate tokio_service;
extern crate rand;

#[macro_use]
extern crate log;
extern crate env_logger;

mod support;

use futures::{Future, oneshot};
use futures::stream::{self, Stream, Receiver};
use support::mock;
use tokio_service::Service;
use tokio_proto::Message;
use tokio_proto::multiplex::{self, Frame, RequestId};
use tokio_core::reactor::Core;
use std::io;
use std::thread;
use std::cell::RefCell;
use std::sync::mpsc;

// The message type is a static string for both the request and response
type Msg = &'static str;

// The body stream is a stream of u32 values
type Body = Receiver<u32, io::Error>;

// Frame written to the transport
type InFrame = Frame<Msg, u32, io::Error>;
type OutFrame = Frame<Message<Msg, Body>, u32, io::Error>;

// Transport handle
type TransportHandle = mock::TransportHandle<InFrame, OutFrame>;

// Client handle
type Client = tokio_proto::Client<&'static str, Message<Msg, Body>, Body, io::Error>;

#[test]
fn test_ping_pong_close() {
    run(|mock, service| {
        mock.allow_write();

        let pong = service.call(Message::WithoutBody("ping"));
        let wr = mock.next_write();
        assert_eq!(Some(0), wr.request_id());
        assert_eq!("ping", wr.unwrap_msg());

        mock.send(msg(0, "pong"));
        assert_eq!("pong", pong.wait().unwrap().into_inner());

        mock.send(multiplex::Frame::Done);
        mock.allow_and_assert_drop();
    });
}

#[test]
fn test_error_on_response() {
    run(|mock, service| {
        mock.allow_write();

        let pong = service.call(Message::WithoutBody("ping"));

        let wr = mock.next_write();
        assert_eq!(Some(0), wr.request_id());
        assert_eq!("ping", wr.unwrap_msg());

        mock.send(multiplex::Frame::Error(0, io::Error::new(io::ErrorKind::Other, "nope")));
        assert_eq!(io::ErrorKind::Other, pong.wait().unwrap_err().kind());

        mock.send(multiplex::Frame::Done);
        mock.allow_and_assert_drop();
    });
}

#[test]
fn drop_client_while_streaming_body() {
    run(|mock, service| {
        mock.allow_write();

        let pong = service.call(Message::WithoutBody("ping"));

        let wr = mock.next_write();
        assert_eq!(Some(0), wr.request_id());
        assert_eq!("ping", wr.unwrap_msg());

        mock.send(msg_with_body(0, "pong"));

        let mut pong = pong.wait().unwrap();
        assert_eq!("pong", &pong.get_ref()[..]);

        let rx = pong.take_body().unwrap();

        // Drop the client
        drop(service);

        // Send body frames
        for i in 0..3 {
            mock.allow_write();
            mock.send(multiplex::Frame::Body(0, Some(i)));
        }

        mock.allow_write();
        mock.send(multiplex::Frame::Body(0, None));

        mock.send(multiplex::Frame::Done);
        mock.allow_write();

        let body: Vec<u32> = rx.wait().map(|i| i.unwrap()).collect();
        assert_eq!(&[0, 1, 2], &body[..]);

        mock.assert_drop();
    });
}

fn msg(request_id: RequestId, msg: Msg) -> OutFrame {
    Frame::Message(request_id, Message::WithoutBody(msg))
}

fn msg_with_body(request_id: RequestId, msg: Msg) -> OutFrame {
    let (tx, rx) = stream::channel();
    Frame::MessageWithBody(request_id, Message::WithBody(msg, rx), tx)
}

/// Setup a reactor running a multiplex::Client and a mock transport. Yields the
/// mock transport handle to the function.
fn run<F>(f: F) where F: FnOnce(TransportHandle, Client) {
    let _ = ::env_logger::init();

    let (tx, rx) = oneshot();
    let (tx2, rx2) = mpsc::channel();
    let t = thread::spawn(move || {
        let mut lp = Core::new().unwrap();
        let handle = lp.handle();
        let (mock, new_transport) = mock::transport::<InFrame, OutFrame>(handle.clone());

        let transport = new_transport.new_transport().unwrap();
        let transport = RefCell::new(Some(transport));
        let new_transport = move || Ok(transport.borrow_mut().take().unwrap());

        let service = multiplex::connect(new_transport, &handle);

        tx2.send((mock, service)).unwrap();
        lp.run(rx)
    });

    let (mock, service) = rx2.recv().unwrap();

    f(mock, service);

    tx.complete(());
    t.join().unwrap().unwrap();
}
