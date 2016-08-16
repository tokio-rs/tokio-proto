extern crate tokio;

use tokio::reactor::{Tick, Task, Reactor, schedule};
use tokio::tcp::TcpListener;

struct Listener {
    socket: TcpListener,
}

impl Task for Listener {
    fn tick(&mut self) -> std::io::Result<Tick> {
        // As long as there are sockets to accept, accept and process them
        while let Some(socket) = try!(self.socket.accept()) {
            // Do something with the socket
            println!("{:#?}", socket);
        }

        Ok(Tick::WouldBlock)
    }
}

pub fn main() {
    let reactor = Reactor::default().unwrap();

    // Run a closure on the reactor
    reactor.handle().oneshot(|| {
        let addr = "0.0.0.0:4000".parse().unwrap();
        let listener = try!(TcpListener::bind(&addr));

        // Schedule the task managing the listener
        schedule(Listener { socket: listener })
            .unwrap();

        Ok(())
    });

    println!("Listening on port 4000. Curl me!");

    reactor.run()
        .unwrap();
}
