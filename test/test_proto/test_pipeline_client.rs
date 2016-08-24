use support::{self, await, mock};
use tokio::Service;
use tokio::proto::pipeline;
use tokio::reactor::Reactor;
use tokio::util::future::{self, Receiver};
use std::io;

// Transport handle
type TransportHandle = mock::TransportHandle<Frame, Frame>;

// Client handle
type ClientHandle = pipeline::ClientHandle<mock::Transport<Frame, Frame>, Body, io::Error>;

// In frame
type Frame = pipeline::Frame<&'static str, io::Error, u32>;

// Body stream
type Body = Receiver<u32, io::Error>;

#[test]
fn test_ping_pong_close() {
    run(|mock, service| {
        mock.allow_write();

        let pong = service.call(pipeline::Message::WithoutBody("ping"));
        assert_eq!("ping", mock.next_write().unwrap_msg());

        mock.send(pipeline::Frame::Message("pong"));
        assert_eq!("pong", await(pong).unwrap());

        mock.send(pipeline::Frame::Done);
        mock.allow_and_assert_drop();
    });
}

#[test]
#[ignore]
fn test_response_ready_before_request_sent() {
    run(|mock, service| {
        mock.send(pipeline::Frame::Message("pong"));

        support::sleep_ms(20);

        let pong = service.call(pipeline::Message::WithoutBody("ping"));

        assert_eq!("pong", await(pong).unwrap());
    });
}

#[test]
fn test_streaming_request_body() {
    run(|mock, service| {
        let (mut tx, rx) = future::channel();

        mock.allow_write();
        let pong = service.call(pipeline::Message::WithBody("ping", rx));

        assert_eq!("ping", mock.next_write().unwrap_msg());

        for i in 0..3 {
            mock.allow_write();
            tx = await(tx.send(Ok(i)).unwrap()).unwrap();
            assert_eq!(Some(i), mock.next_write().unwrap_body());
        }

        mock.allow_write();
        drop(tx);
        assert_eq!(None, mock.next_write().unwrap_body());

        mock.send(pipeline::Frame::Message("pong"));
        assert_eq!("pong", await(pong).unwrap());

        mock.send(pipeline::Frame::Done);
        mock.allow_and_assert_drop();
    });
}

#[test]
#[ignore]
fn test_streaming_response_body() {
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
