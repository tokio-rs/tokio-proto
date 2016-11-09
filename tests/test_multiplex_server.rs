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
use support::FnService;

use futures::{Future, finished, oneshot};
use futures::stream::{self, Stream};
use tokio_proto::Message;
use tokio_proto::multiplex::{Frame};
use rand::Rng;
use std::io;
use std::sync::{mpsc, Arc, Mutex};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::Duration;

#[test]
fn test_immediate_done() {
    let service = FnService::new(|_| {
        finished(Message::WithoutBody("goodbye"))
    });

    mux::run(service, |mock| {
        mock.send(mux::done());
        mock.allow_and_assert_drop();
    });
}

#[test]
fn test_immediate_writable_echo() {
    let service = FnService::new(|req| {
        assert_eq!(req, "hello");
        finished(Message::WithoutBody("goodbye"))
    });

    mux::run(service, |mock| {
        mock.allow_write();
        mock.send(mux::message(0, "hello"));

        let wr = mock.next_write();
        assert_eq!(wr.request_id(), Some(0));
        assert_eq!(wr.unwrap_msg(), "goodbye");

        mock.send(Frame::Done);
        mock.allow_and_assert_drop();
    });
}

#[test]
fn test_immediate_writable_delayed_response_echo() {
    let (c, fut) = oneshot();
    let fut = Mutex::new(Some(fut));

    let service = FnService::new(move |req| {
        assert_eq!(req, "hello");
        fut.lock().unwrap().take().unwrap().then(|r| r.unwrap())
    });

    mux::run(service, |mock| {
        mock.allow_write();
        mock.send(mux::message(0, "hello"));

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
    let service = FnService::new(|req| {
        assert_eq!(req, "hello");
        finished(Message::WithoutBody("goodbye"))
    });

    mux::run(service, |mock| {
        mock.send(mux::message(0, "hello"));

        support::sleep_ms(20);

        mock.allow_write();

        let wr = mock.next_write();
        assert_eq!(wr.request_id(), Some(0));
        assert_eq!(wr.unwrap_msg(), "goodbye");
    });
}

#[test]
fn test_same_order_multiplexing() {
    let (tx, rx) = mpsc::channel();
    let tx = Mutex::new(tx);

    let service = FnService::new(move |_| {
        let (c, fut) = oneshot();
        tx.lock().unwrap().send(c).unwrap();
        fut.then(|r| r.unwrap())
    });

    mux::run(service, |mock| {
        // Allow all the writes
        for _ in 0..3 { mock.allow_write() };

        mock.send(mux::message(0, "hello"));
        let c1 = rx.recv().unwrap();

        mock.send(mux::message(1, "hello"));
        let c2 = rx.recv().unwrap();

        mock.send(mux::message(2, "hello"));
        let c3 = rx.recv().unwrap();

        mock.assert_no_write(20);

        c1.complete(Ok(Message::WithoutBody("one")));
        thread::sleep(Duration::from_millis(20));

        c2.complete(Ok(Message::WithoutBody("two")));
        thread::sleep(Duration::from_millis(20));

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
    let (tx, rx) = mpsc::channel();
    let tx = Mutex::new(tx);

    let service = FnService::new(move |_| {
        let (c, fut) = oneshot();
        tx.lock().unwrap().send(c).unwrap();
        fut.then(|r| r.unwrap())
    });

    mux::run(service, |mock| {
        // Allow all the writes
        for _ in 0..3 { mock.allow_write() };

        mock.send(mux::message(0, "hello"));
        let c1 = rx.recv().unwrap();

        mock.send(mux::message(1, "hello"));
        let c2 = rx.recv().unwrap();

        mock.send(mux::message(2, "hello"));
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
    let (tx, rx) = mpsc::channel();
    let tx = Mutex::new(tx);

    let service = FnService::new(move |req: Message<mux::Head, mux::Body>| {
        tx.lock().unwrap().send(req.clone()).unwrap();
        finished(Message::WithoutBody(*req.get_ref()))
    });

    mux::run(service, |mock| {
        mock.send(mux::message(0, "one"));
        mock.send(mux::message(1, "two"));
        mock.send(mux::message(2, "three"));

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
    let service = FnService::new(move |_| {
        finished(Message::WithoutBody("goodbye"))
    });

    mux::run(service, |mock| {
        mock.send(mux::message(0, "hello"));

        mock.allow_and_assert_flush();
        mock.allow_and_assert_flush();

        mock.allow_write();
        assert_eq!("goodbye", mock.next_write().unwrap_msg());

        mock.send(Frame::Done);
        mock.assert_drop();
    });
}

#[test]
fn test_reaching_max_in_flight_requests() {
    use futures::Oneshot;

    let (tx, rx) = mpsc::channel::<Oneshot<Result<Message<mux::Head, mux::BodyBox>, io::Error>>>();
    let rx = Arc::new(Mutex::new(rx));

    let c1 = Arc::new(AtomicUsize::new(0));
    let c2 = c1.clone();

    let service = FnService::new(move |_| {
        c2.fetch_add(1, Ordering::SeqCst);
        let fut = rx.lock().unwrap().recv().unwrap();
        fut.map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "broken pipe"))
           .and_then(|res| res)
    });

    let mut responses = vec![];

    mux::run(service, |mock| {
        for i in 0..33 {
            let (c, resp) = oneshot();
            tx.send(resp).unwrap();
            responses.push((i, c));
            mock.send(mux::message(i, "request"));
        }

        // wait a bit
        mock.assert_no_write(100);

        // Only 32 requests processed
        assert_eq!(32, c1.load(Ordering::SeqCst));

        // Pick one from the first 32 requests to complete.
        rand::thread_rng().shuffle(&mut responses[0..32]);
        let (i, c) = responses.remove(0);

        c.complete(Ok(Message::WithoutBody("zomg")));

        mock.assert_no_write(50);

        // Next request not yet processed
        assert_eq!(32, c1.load(Ordering::SeqCst));

        // allow the write
        mock.allow_write();

        // Read the response
        let wr = mock.next_write();
        assert_eq!(Some(i), wr.request_id());
        assert_eq!("zomg", wr.unwrap_msg());

        mock.assert_no_write(50);

        // Next request is processed
        assert_eq!(33, c1.load(Ordering::SeqCst));

        // Complete pending requests
        for (i, c) in responses.drain(..) {
            mock.allow_write();
            c.complete((Ok(Message::WithoutBody("zomg"))));

            let wr = mock.next_write();
            assert_eq!(Some(i), wr.request_id());
            assert_eq!("zomg", wr.unwrap_msg());
        }

        mock.send(Frame::Done);
        mock.assert_drop();
    });
}

#[test]
fn test_basic_streaming_response_body() {
    let (tx, rx) = stream::channel::<u32, io::Error>();
    let rx = Mutex::new(Some(rx));

    let service = FnService::new(move |req| {
        assert_eq!(req, "want-body");

        let body = rx.lock().unwrap().take().unwrap();
        finished(Message::WithBody("hi2u", Box::new(body) as mux::BodyBox))
    });

    mux::run(service, |mock| {
        mock.allow_write();
        mock.send(mux::message(3, "want-body"));

        let wr = mock.next_write();
        assert_eq!(Some(3), wr.request_id());
        assert_eq!(wr.unwrap_msg(), "hi2u");

        mock.assert_no_write(20);

        // Allow the write, then send the message
        mock.allow_write();
        let tx = tx.send(Ok(1)).wait().ok().unwrap();
        let wr = mock.next_write();
        assert_eq!(Some(3), wr.request_id());
        assert_eq!(Some(1), wr.unwrap_body());

        // Send the message then allow the write
        let tx = tx.send(Ok(2)).wait().ok().unwrap();
        mock.assert_no_write(20);
        mock.allow_write();
        let wr = mock.next_write();
        assert_eq!(Some(3), wr.request_id());
        assert_eq!(Some(2), wr.unwrap_body());

        mock.assert_no_write(20);
        mock.allow_write();
        mock.assert_no_write(20);
        drop(tx);

        let wr = mock.next_write();
        assert_eq!(Some(3), wr.request_id());
        assert_eq!(None, wr.unwrap_body());

        // Alright, clean shutdown
        mock.send(Frame::Done);
        mock.allow_and_assert_drop();
    });
}

#[test]
fn test_basic_streaming_request_body_read_then_respond() {
    let (tx, rx) = mpsc::channel();
    let tx = Arc::new(Mutex::new(tx));

    let service = FnService::new(move |mut req: Message<mux::Head, mux::Body>| {
        assert_eq!(req, "have-body");

        let body = req.take_body().unwrap();
        let tx = tx.clone();

        body.for_each(move |chunk| {
            tx.lock().unwrap().send(chunk).unwrap();
            Ok(())
        }).and_then(|_| {
            Ok(Message::WithoutBody("hi2u"))
        })
    });

    mux::run(service, |mock| {
        mock.allow_write();
        mock.send(mux::message_with_body(2, "have-body"));

        for i in 0..5 {
            // No write yet
            mock.assert_no_write(20);

            // Send a body chunk
            mock.send(Frame::Body { id: 2, chunk: Some(i) });

            // Assert service processed chunk
            assert_eq!(i, rx.recv().unwrap());
        }

        // Send end-of-stream notification
        mock.send(Frame::Body { id: 2, chunk: None });

        let wr = mock.next_write();
        assert_eq!(Some(2), wr.request_id());
        assert_eq!("hi2u", wr.unwrap_msg());

        // Clean shutdown
        mock.send(Frame::Done);
        mock.allow_and_assert_drop();
    });
}

#[test]
fn test_interleaving_request_body_chunks() {
    let (tx, rx) = mpsc::channel();
    let tx = Mutex::new(tx);
    let cnt = AtomicUsize::new(0);

    let service = FnService::new(move |mut req: Message<mux::Head, mux::Body>| {
        let body = req.take_body().unwrap();
        let tx = tx.lock().unwrap().clone();
        let i = cnt.fetch_add(1, Ordering::Relaxed);

        assert_eq!(req, &format!("have-body-{}", i));

        body.for_each(move |chunk| {
            tx.send((i, chunk)).unwrap();
            Ok(())
        }).and_then(|_| {
            Ok(Message::WithoutBody("hi2u"))
        })
    });

    mux::run(service, |mock| {
        mock.send(mux::message_with_body(2, "have-body-0"));
        mock.send(mux::message_with_body(4, "have-body-1"));

        // The write must be allowed in order to process the bodies
        mock.allow_write();

        for i in 0..5 {
            if i % 2 == 0 {
                // No write yet
                mock.assert_no_write(20);

                // Send a body chunk
                mock.send(Frame::Body { id: 2, chunk: Some(i) });
                assert_eq!((0, i), rx.recv().unwrap());

                mock.send(Frame::Body { id: 4, chunk: Some(i) });
                assert_eq!((1, i), rx.recv().unwrap());
            } else {
                // No write yet
                mock.assert_no_write(20);

                mock.send(Frame::Body { id: 4, chunk: Some(i) });
                assert_eq!((1, i), rx.recv().unwrap());

                // Send a body chunk
                mock.send(Frame::Body { id: 2, chunk: Some(i) });
                assert_eq!((0, i), rx.recv().unwrap());
            }
        }

        // Send end-of-stream notification
        mock.send(Frame::Body { id: 2, chunk: None });

        let wr = mock.next_write();
        assert_eq!(Some(2), wr.request_id());
        assert_eq!("hi2u", wr.unwrap_msg());

        mock.allow_write();
        mock.send(Frame::Body { id: 4, chunk: None });

        let wr = mock.next_write();
        assert_eq!(Some(4), wr.request_id());
        assert_eq!("hi2u", wr.unwrap_msg());

        // Clean shutdown
        mock.send(Frame::Done);
        mock.allow_and_assert_drop();
    });
}

#[test]
fn test_interleaving_response_body_chunks() {
}

#[test]
fn test_transport_provides_invalid_request_ids() {
}

#[test]
fn test_reaching_max_buffered_frames() {
}

#[test]
fn test_read_error_as_first_frame() {
    let service = FnService::new(|_| {
        // Makes the compiler happy
        if true {
            panic!("should not be called");
        }

        finished(Message::WithoutBody("nope"))
    });

    mux::run(service, |mock| {
        mock.allow_write();
        mock.send(Frame::Error { id: 1, error: io::Error::new(io::ErrorKind::Other, "boom") });

        mock.send(Frame::Done);
        mock.allow_and_assert_drop();
    });
}

#[test]
fn test_read_error_during_stream() {
}

#[test]
fn test_error_handling_before_message_dispatched() {
    /*
    let service = FnService::new(|_| {
        unimplemented!();
    });
    */
}
