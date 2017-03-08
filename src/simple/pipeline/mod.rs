//! Pipelined RPC protocols.
//!
//! See the crate-level docs for an overview.

mod client;
pub use self::client::ClientProto;
pub use self::client::ClientService;

mod server;
pub use self::server::ServerProto;

/// A marker used to flag protocols as being pipelined RPC.
///
/// This is an implementation detail; to actually implement a protocol,
/// implement the `ClientProto` or `ServerProto` traits in this module.
#[derive(Debug)]
pub struct Pipeline;

// This is a submodule so that `LiftTransport` can be marked `pub`, to satisfy
// the no-private-in-public checker.
mod lift {
    use std::io;
    use std::marker::PhantomData;

    use streaming::pipeline::{Frame, Transport};
    use futures::{Future, Stream, Sink, StartSend, Poll, Async, AsyncSink};

    // Lifts an implementation of RPC-style transport to streaming-style transport
    pub struct LiftTransport<T, E>(pub T, pub PhantomData<E>);

    // Lifts the Bind from the underlying transport
    pub struct LiftBind<A, F, E> {
        fut: F,
        marker: PhantomData<(A, E)>,
    }

    impl<E, T: Stream<Error = io::Error>> Stream for LiftTransport<T, E> {
        type Item = Frame<T::Item, (), E>;
        type Error = io::Error;

        fn poll(&mut self) -> Poll<Option<Self::Item>, io::Error> {
            let item = try_ready!(self.0.poll());
            Ok(item.map(|msg| {
                Frame::Message { message: msg, body: false }
            }).into())
        }
    }

    impl<E, T: Sink<SinkError = io::Error>> Sink for LiftTransport<T, E> {
        type SinkItem = Frame<T::SinkItem, (), E>;
        type SinkError = io::Error;

        fn start_send(&mut self, request: Self::SinkItem)
                      -> StartSend<Self::SinkItem, io::Error> {
            if let Frame::Message { message, body } = request {
                if !body {
                    match try!(self.0.start_send(message)) {
                        AsyncSink::Ready => return Ok(AsyncSink::Ready),
                        AsyncSink::NotReady(msg) => {
                            let msg = Frame::Message { message: msg, body: false };
                            return Ok(AsyncSink::NotReady(msg))
                        }
                    }
                }
            }
            Err(io::Error::new(io::ErrorKind::Other, "no support for streaming"))
        }

        fn poll_complete(&mut self) -> Poll<(), io::Error> {
            self.0.poll_complete()
        }

        fn close(&mut self) -> Poll<(), io::Error> {
            self.0.close()
        }
    }

    impl<T, E: 'static> Transport for LiftTransport<T, E>
        where T: 'static + Stream<Error = io::Error> + Sink<SinkError = io::Error>
    {}

    impl<A, F, E> LiftBind<A, F, E> {
        pub fn lift(f: F) -> LiftBind<A, F, E> {
            LiftBind {
                fut: f,
                marker: PhantomData,
            }
        }
    }

    impl<A, F, E> Future for LiftBind<A, F, E> where F: Future<Error = io::Error> {
        type Item = LiftTransport<F::Item, E>;
        type Error = io::Error;

        fn poll(&mut self) -> Poll<Self::Item, io::Error> {
            Ok(Async::Ready(LiftTransport(try_ready!(self.fut.poll()), PhantomData)))
        }
    }
}
