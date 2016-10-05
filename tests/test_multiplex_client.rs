extern crate futures;
extern crate tokio_core;
extern crate tokio_proto;
extern crate tokio_service;
extern crate rand;

#[macro_use]
extern crate log;
extern crate env_logger;

mod support;

use support::multiplex as mux;

use futures::{Future};
use futures::stream::{Stream};
use tokio_service::Service;
use tokio_proto::Message;
use tokio_proto::multiplex;
use std::io;

#[test]
fn test_ping_pong_close() {
    mux::client(|mock, service| {
        mock.allow_write();

        let pong = service.call(Message::WithoutBody("ping"));
        let wr = mock.next_write();
        assert_eq!(Some(0), wr.request_id());
        assert_eq!("ping", wr.unwrap_msg());

        mock.send(mux::message(0, "pong"));
        assert_eq!("pong", pong.wait().unwrap().into_inner());

        mock.send(multiplex::Frame::Done);
        mock.allow_and_assert_drop();
    });
}

#[test]
fn test_error_on_response() {
    mux::client(|mock, service| {
        mock.allow_write();

        let pong = service.call(Message::WithoutBody("ping"));

        let wr = mock.next_write();
        assert_eq!(Some(0), wr.request_id());
        assert_eq!("ping", wr.unwrap_msg());

        mock.send(multiplex::Frame::Error {
            id: 0,
            error: io::Error::new(io::ErrorKind::Other, "nope"),
        });

        assert_eq!(io::ErrorKind::Other, pong.wait().unwrap_err().kind());

        mock.send(multiplex::Frame::Done);
        mock.allow_and_assert_drop();
    });
}

#[test]
fn drop_client_while_streaming_body() {
    mux::client(|mock, service| {
        mock.allow_write();

        let pong = service.call(Message::WithoutBody("ping"));

        let wr = mock.next_write();
        assert_eq!(Some(0), wr.request_id());
        assert_eq!("ping", wr.unwrap_msg());

        mock.send(mux::message_with_body(0, "pong"));

        let mut pong = pong.wait().unwrap();
        assert_eq!("pong", &pong.get_ref()[..]);

        let rx = pong.take_body().unwrap();

        // Drop the client
        drop(service);

        // Send body frames
        for i in 0..3 {
            mock.allow_write();
            mock.send(mux::body(0, Some(i)));
        }

        mock.allow_write();
        mock.send(mux::body(0, None));

        mock.send(mux::done());
        mock.allow_write();

        let body: Vec<u32> = rx.wait().map(|i| i.unwrap()).collect();
        assert_eq!(&[0, 1, 2], &body[..]);

        mock.assert_drop();
    });
}
