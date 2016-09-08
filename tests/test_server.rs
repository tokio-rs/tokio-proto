extern crate futures;
extern crate tokio_core;
extern crate tokio_proto;
extern crate rand;

#[macro_use]
extern crate log;
extern crate env_logger;

mod support;

use futures::{oneshot, Future, Poll, Async};
use tokio_proto::server;
use tokio_core::reactor::Core;
use std::io;
use std::net::{SocketAddr, TcpStream};
use std::sync::mpsc;
use std::thread;

#[test]
fn test_accepting_multiple_sockets() {
    struct Connection;

    impl Future for Connection {
        type Item = ();
        type Error = io::Error;

        fn poll(&mut self) -> Poll<(), io::Error> {
            Ok(Async::Ready(()))
        }
    }

    let (tx, rx) = mpsc::channel();
    let address: SocketAddr = "127.0.0.1:14564".parse().unwrap();
    let address2 = address.clone();
    let t = thread::spawn(move || {
        let mut lp = Core::new().unwrap();
        let (tx2, rx2) = oneshot();
        server::listen(&lp.handle(), address2, |_| Ok(Connection)).unwrap();
        tx.send(tx2).unwrap();
        lp.run(rx2)
    });


    let tx = rx.recv().unwrap();

    let _ = TcpStream::connect(&address).unwrap();

    support::sleep_ms(100);

    let _ = TcpStream::connect(&address).unwrap();

    tx.complete(());
    t.join().unwrap().unwrap();
}
