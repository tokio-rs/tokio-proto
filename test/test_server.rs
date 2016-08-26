use std::io;
use std::net::{SocketAddr, TcpStream};
use std::sync::mpsc;
use std::thread;

use futures::{oneshot, Future, Poll};
use tokio::server;
use tokio_core::Loop;

use support;

#[test]
fn test_accepting_multiple_sockets() {
    struct Connection;

    impl Future for Connection {
        type Item = ();
        type Error = io::Error;

        fn poll(&mut self) -> Poll<(), io::Error> {
            Poll::Ok(())
        }
    }

    let (tx, rx) = mpsc::channel();
    let t = thread::spawn(move || {
        let mut lp = Loop::new().unwrap();
        let (tx2, rx2) = oneshot();
        tx.send((lp.handle(), tx2)).unwrap();
        lp.run(rx2)
    });


    let address: SocketAddr = "127.0.0.1:14564".parse().unwrap();

    let (handle, tx) = rx.recv().unwrap();
    server::listen(handle, address.clone(), |_| Ok(Connection)).forget();

    let _ = TcpStream::connect(&address).unwrap();

    support::sleep_ms(100);

    let _ = TcpStream::connect(&address).unwrap();

    tx.complete(());
    t.join().unwrap().unwrap();
}
