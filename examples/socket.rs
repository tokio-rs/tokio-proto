extern crate futures;
extern crate tokio_proto;
extern crate tokio_core;

use futures::Future;
use futures::stream::Stream;
use tokio_core::Loop;

pub fn main() {
    let mut lp = Loop::new().unwrap();

    let addr = "0.0.0.0:4000".parse().unwrap();
    let listener = lp.handle().tcp_listen(&addr);
    let srv = listener.and_then(|l| {
        l.incoming().for_each(|socket| {
            // Do something with the socket
            println!("{:#?}", socket);
            Ok(())
        })
    });

    lp.run(srv).unwrap();
}
