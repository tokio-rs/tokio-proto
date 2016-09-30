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
use futures::{Future, oneshot};
use support::mock;
use tokio_service::Service;
use tokio_proto::{multiplex, Message};
use tokio_core::reactor::Core;
use std::io;
use std::thread;
use std::cell::RefCell;
use std::sync::mpsc;

// Transport handle
type TransportHandle = mock::TransportHandle<Frame, Frame>;

// Client handle
type Client = tokio_proto::Client<&'static str, &'static str, Body, io::Error>;

// In frame
type Frame = multiplex::Frame<&'static str, u32, io::Error>;

// Body stream
type Body = Receiver<u32, io::Error>;

#[test]
fn test_ping_pong_close() {
    run(|mock, service| {
        mock.allow_write();

        let pong = service.call(Message::WithoutBody("ping"));
        let wr = mock.next_write();
        assert_eq!(Some(0), wr.request_id());
        assert_eq!("ping", wr.unwrap_msg());

        mock.send(multiplex::Frame::Message(0, "pong"));
        assert_eq!("pong", pong.wait().unwrap());

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

/// Setup a reactor running a multiplex::Client and a mock transport. Yields the
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
