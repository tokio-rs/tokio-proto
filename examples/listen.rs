extern crate futures;
extern crate tokio_proto;
extern crate tokio_core;

use futures::Future;
use futures::stream::Stream;
use tokio_core::Loop;

pub fn main() {
    // Create a new loop
    let mut lp = Loop::new().unwrap();

    // Bind to port 4000
    let addr = "0.0.0.0:4000".parse().unwrap();

    // Create the new TCP listener
    let listener = lp.handle().tcp_listen(&addr);

    let srv = listener.and_then(|l| {
        // Accept each incoming connection
        l.incoming().for_each(|socket| {
            // Do something with the socket
            println!("{:#?}", socket);
            Ok(())
        })
    });

    println!("listening on {:?}", addr);

    lp.run(srv).unwrap();
}
