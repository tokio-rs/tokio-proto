use support::{self, await, mock};
use tokio::Service;
use tokio::proto::pipeline::{self, Frame};
use tokio::reactor::Reactor;
use tokio::util::future::{self, Receiver};
use std::io;

// Transport handle
type TransportHandle = mock::TransportHandle<InFrame, OutFrame>;

// Client handle
type ClientHandle = pipeline::ClientHandle<mock::Transport<InFrame, OutFrame>, Body, io::Error>;

// In frame
type InFrame = pipeline::Frame<&'static str, io::Error, u32>;

// Out frame
type OutFrame = pipeline::OutFrame<&'static str, io::Error, u32>;

// Body stream
type Body = Receiver<u32, io::Error>;

#[test]
fn test_ping_pong_close() {
    run(|mock, service| {
        mock.allow_write();

        let pong = service.call(("ping", None));
        assert_eq!("ping", mock.next_write().unwrap_msg());

        mock.send(Frame::Message(("pong", None)));
        assert_eq!("pong", await(pong).unwrap());

        mock.send(Frame::Done);
        mock.allow_and_assert_drop();
    });
}

#[test]
#[ignore]
fn test_response_ready_before_request_sent() {
    run(|mock, service| {
        mock.send(Frame::Message(("pong", None)));

        support::sleep_ms(20);

        let pong = service.call(("ping", None));

        assert_eq!("pong", await(pong).unwrap());
    });
}

#[test]
fn test_streaming_request_body() {
    run(|mock, service| {
        let (mut tx, rx) = future::channel();

        mock.allow_write();
        let pong = service.call(("ping", Some(rx)));

        assert_eq!("ping", mock.next_write().unwrap_msg());

        for i in 0..3 {
            mock.allow_write();
            tx = await(tx.send(Ok(i)).unwrap()).unwrap();
            assert_eq!(Some(i), mock.next_write().unwrap_body());
        }

        mock.allow_write();
        drop(tx);
        assert_eq!(None, mock.next_write().unwrap_body());

        mock.send(Frame::Message(("pong", None)));
        assert_eq!("pong", await(pong).unwrap());

        mock.send(Frame::Done);
        mock.allow_and_assert_drop();
    });
}

/// Setup a reactor running a pipeline::Client and a mock transport. Yields the
/// mock transport handle to the function.
fn run<F>(f: F) where F: FnOnce(TransportHandle, ClientHandle) {
    use take::Take;

    let _ = ::env_logger::init();
    let r = Reactor::default().unwrap();
    let h = r.handle();

    let (mock, new_transport) = mock::transport();

    // Spawn the reactor
    r.spawn();

    let service = pipeline::connect(&h, Take::new(|| {
        new_transport.new_transport()
    }));

    f(mock, service);


    h.shutdown();
}
