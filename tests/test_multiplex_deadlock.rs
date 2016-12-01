/*
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

use tokio_proto::streaming::Message;
use tokio_proto::streaming::multiplex::{self, Frame};
use futures::{stream, Async, Poll};
use std::io;

#[test]
fn test_write_requires_flush() {

    // Create a custom Transport middleware that requires a flush before
    // enabling reading

    struct Transport<T: multiplex::Transport> {
        upstream: T,
        buffer: Option<Frame<T::In, T::BodyIn, T::Error>>,
    }

    impl<T: multiplex::Transport> Transport<T> {
        fn new(upstream: T) -> Transport<T> {
            Transport {
                upstream: upstream,
                buffer: None,
            }
        }
    }

    impl<T> multiplex::Transport for Transport<T>
        where T: multiplex::Transport,
    {
        type In = T::In;
        type Out = T::Out;
        type BodyIn = T::BodyIn;
        type BodyOut = T::BodyOut;
        type Error = T::Error;

        fn poll_read(&mut self) -> Async<()> {
            self.upstream.poll_read()
        }

        fn read(&mut self) -> Poll<Frame<Self::Out, Self::BodyOut, Self::Error>, io::Error> {
            self.upstream.read()
        }

        fn poll_write(&mut self) -> Async<()> {
            if self.buffer.is_none() {
                return Async::Ready(());
            }

            Async::NotReady
        }

        fn write(&mut self, message: Frame<Self::In, Self::BodyIn, Self::Error>) -> Poll<(), io::Error> {
            assert!(self.poll_write().is_ready());
            self.buffer = Some(message);
            Ok(Async::Ready(()))
        }

        fn flush(&mut self) -> Poll<(), io::Error> {
            if self.buffer.is_some() {
                if !self.upstream.poll_write().is_ready() {
                    return Ok(Async::NotReady);
                }

                let msg = self.buffer.take().unwrap();
                try!(self.upstream.write(msg));
            }

            self.upstream.flush()
        }
    }

    // Define a simple service that just finishes immediately
    let service = simple_service(|_| {
        let body = vec![0, 1, 2].into_iter().map(Ok);
        let body: mux::BodyBox = Box::new(stream::iter(body));

        let resp = Message::WithBody("goodbye", body);

        futures::finished(resp)
    });

    // Expect a ping pong
    mux::run_with_transport(service, Transport::new, |mock| {
        mock.allow_write();
        mock.send(mux::message(0, "hello"));

        let wr = mock.next_write();
        assert_eq!(wr.request_id(), Some(0));
        assert_eq!(wr.unwrap_msg(), "goodbye");

        for i in 0..3 {
            mock.allow_write();
            let wr = mock.next_write();
            assert_eq!(wr.request_id(), Some(0));
            assert_eq!(wr.unwrap_body(), Some(i));
        }

        mock.allow_write();
        let wr = mock.next_write();
        assert_eq!(wr.request_id(), Some(0));
        assert_eq!(wr.unwrap_body(), None);

        mock.send(Frame::Done);
        mock.allow_and_assert_drop();
    });
}
*/
