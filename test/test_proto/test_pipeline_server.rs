use support::{self, mock};
use tokio::{self, Service};
use tokio::proto::pipeline::{self, Frame};
use tokio::reactor::{self, Reactor};
use tokio::util::future;
use futures::{failed, finished};
use std::io;
use std::sync::{mpsc, Mutex};

#[test]
fn test_immediate_done() {
    let service = tokio::simple_service(|req| {
        finished::<&'static str, io::Error>(req)
    });

    run(service, |mock| {
        mock.send(Frame::Done);
        mock.allow_and_assert_drop();
    });
}

#[test]
fn test_immediate_writable_echo() {
    let service = tokio::simple_service(|req| {
        assert_eq!("hello", req);
        finished::<&'static str, io::Error>(req)
    });

    run(service, |mock| {
        mock.allow_write();
        mock.send(Frame::Message("hello"));
        assert_eq!("hello", mock.next_write().unwrap_msg());

        mock.send(Frame::Done);
        mock.allow_and_assert_drop();
    });
}

#[test]
fn test_immediate_writable_delayed_response_echo() {
    let (c, fut) = future::pair::<&'static str, io::Error>();
    let fut = Mutex::new(Some(fut));

    let service = tokio::simple_service(move |req| {
        assert_eq!("hello", req);
        fut.lock().unwrap().take().unwrap()
    });

    run(service, |mock| {
        mock.allow_write();
        mock.send(Frame::Message("hello"));

        support::sleep_ms(20);
        c.complete("goodbye");

        assert_eq!("goodbye", mock.next_write().unwrap_msg());

        mock.send(Frame::Done);
        mock.allow_and_assert_drop();
    });
}

#[test]
fn test_delayed_writable_immediate_response_echo() {
    let service = tokio::simple_service(|req| {
        assert_eq!("hello", req);
        finished::<&'static str, io::Error>(req)
    });

    run(service, |mock| {
        mock.send(pipeline::Frame::Message("hello"));

        support::sleep_ms(20);

        mock.allow_write();
        assert_eq!("hello", mock.next_write().unwrap_msg());
    });
}

#[test]
fn test_pipelining_while_service_is_processing() {
    let (tx, rx) = channel();

    let service = tokio::simple_service(move |_| {
        let (c, fut) = future::pair();
        tx.lock().unwrap().send(c).unwrap();
        fut
    });

    run(service, |mock| {
        // Allow all the writes
        for _ in 0..3 { mock.allow_write() };

        mock.send(Frame::Message("one"));
        let c1 = rx.recv().unwrap();

        mock.send(Frame::Message("two"));
        let c2 = rx.recv().unwrap();

        mock.send(Frame::Message("three"));
        let c3 = rx.recv().unwrap();

        mock.assert_no_write(20);
        c3.complete("three");

        mock.assert_no_write(20);
        c2.complete("two");

        mock.assert_no_write(20);
        c1.complete("one");

        assert_eq!("one", mock.next_write().unwrap_msg());
        assert_eq!("two", mock.next_write().unwrap_msg());
        assert_eq!("three", mock.next_write().unwrap_msg());
    });
}

#[test]
fn test_pipelining_while_transport_not_writable() {
    let (tx, rx) = channel();

    let service = tokio::simple_service(move |req| {
        tx.lock().unwrap().send(req).unwrap();
        finished::<&'static str, io::Error>(req)
    });

    run(service, |mock| {
        mock.send(Frame::Message("one"));
        mock.send(Frame::Message("two"));
        mock.send(Frame::Message("three"));

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
    let service = tokio::simple_service(move |req| {
        finished::<&'static str, io::Error>(req)
    });

    run(service, |mock| {
        mock.send(Frame::Message("hello"));

        mock.allow_and_assert_flush();
        mock.allow_and_assert_flush();

        mock.allow_write();
        assert_eq!("hello", mock.next_write().unwrap_msg());

        mock.send(Frame::Done);
        mock.assert_drop();
    });
}

#[test]
fn test_returning_error_from_service() {
    let service = tokio::simple_service(move |req| {
        failed::<&'static str, io::Error>(io::Error::new(io::ErrorKind::Other, "nope"))
    });

    run(service, |mock| {
        mock.send(Frame::Message("hello"));

        mock.allow_write();
        assert_eq!(io::ErrorKind::Other, mock.next_write().unwrap_err().kind());

        mock.assert_no_write(20);

        mock.send(Frame::Done);
        mock.assert_drop();
    });
}

#[test]
#[ignore]
fn test_returning_would_block_from_service() {
    // Because... it could happen
}

fn channel<T>() -> (Mutex<mpsc::Sender<T>>, mpsc::Receiver<T>) {
    let (tx, rx) = mpsc::channel();
    let tx = Mutex::new(tx);
    (tx, rx)
}

/// Setup a reactor running a pipeline::Server with the given service and a
/// mock transport. Yields the mock transport handle to the function.
fn run<S, F>(service: S, f: F)
    where S: Service<Req = &'static str, Resp = &'static str, Error = io::Error>,
          F: FnOnce(mock::TransportHandle<Frame<&'static str, io::Error>, Frame<&'static str, io::Error>>),
{
    let _ = ::env_logger::init();
    let r = Reactor::default().unwrap();
    let h = r.handle();

    let (mock, new_transport) = mock::transport();

    // Spawn the reactor
    r.spawn();

    h.oneshot(move || {
        let transport = try!(new_transport.new_transport());
        let dispatch = try!(pipeline::Server::new(service, transport));

        try!(reactor::schedule(dispatch));

        Ok(())
    });

    f(mock);


    h.shutdown();
}
