use support;
use tokio::server;
use tokio::reactor::{Reactor, Task, Tick};
use std::io;
use std::net::{SocketAddr, TcpStream};

#[test]
fn test_accepting_multiple_sockets() {
    struct Connection;

    impl Task for Connection {
        fn tick(&mut self) -> io::Result<Tick> {
            Ok(Tick::Final)
        }
    }

    let reactor = Reactor::default().unwrap();
    let address: SocketAddr = "127.0.0.1:14564".parse().unwrap();

    let handle = reactor.handle();
    reactor.spawn();

    server::listen(&handle, address.clone(), |_| Ok(Connection)).unwrap();

    let _ = TcpStream::connect(&address).unwrap();

    support::sleep_ms(100);

    let _ = TcpStream::connect(&address).unwrap();

    handle.shutdown();
}
