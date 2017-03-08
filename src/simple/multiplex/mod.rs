//! Multiplexed RPC protocols.
//!
//! See the crate-level docs for an overview.

mod client;
pub use self::client::ClientProto;
pub use self::client::ClientService;

mod server;
pub use self::server::ServerProto;

/// Identifies a request / response thread
pub type RequestId = u64;

/// A marker used to flag protocols as being multiplexed RPC.
///
/// This is an implementation detail; to actually implement a protocol,
/// implement the `ClientProto` or `ServerProto` traits in this module.
#[derive(Debug)]
pub struct Multiplex;

// This is a submodule so that `LiftTransport` can be marked `pub`, to satisfy
// the no-private-in-public checker.
mod lift {
    use std::io;
    use std::marker::PhantomData;

    use super::RequestId;
    use streaming::multiplex::{Frame, Transport};
    use futures::{Future, Stream, Sink, StartSend, Poll, Async, AsyncSink};

    // Lifts an implementation of RPC-style transport to streaming-style transport
    pub struct LiftTransport<T, E>(pub T, pub PhantomData<E>);

    // Lifts the Bind from the underlying transport
    pub struct LiftBind<A, F, E> {
        fut: F,
        marker: PhantomData<(A, E)>,
    }

    impl<T, InnerItem, E> Stream for LiftTransport<T, E> where
        E: 'static,
        T: Stream<Item = (RequestId, InnerItem), Error = io::Error>,
    {
        type Item = Frame<InnerItem, (), E>;
        type Error = io::Error;

        fn poll(&mut self) -> Poll<Option<Self::Item>, io::Error> {
            let (id, msg) = match try_ready!(self.0.poll()) {
                Some(msg) => msg,
                None => return Ok(None.into()),
            };
            Ok(Some(Frame::Message {
                message: msg,
                body: false,
                solo: false,
                id: id,
            }).into())
        }
    }

    impl<T, InnerSink, E> Sink for LiftTransport<T, E> where
        E: 'static,
        T: Sink<SinkItem = (RequestId, InnerSink), SinkError = io::Error>
    {
        type SinkItem = Frame<InnerSink, (), E>;
        type SinkError = io::Error;

        fn start_send(&mut self, request: Self::SinkItem)
                      -> StartSend<Self::SinkItem, io::Error> {
            if let Frame::Message { message, id, body, solo } = request {
                if !body && !solo {
                    match try!(self.0.start_send((id, message))) {
                        AsyncSink::Ready => return Ok(AsyncSink::Ready),
                        AsyncSink::NotReady((id, msg)) => {
                            let msg = Frame::Message {
                                message: msg,
                                id: id,
                                body: false,
                                solo: false,
                            };
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

    impl<T, InnerItem, InnerSink, E> Transport<()> for LiftTransport<T, E> where
        E: 'static,
        T: 'static,
        T: Stream<Item = (RequestId, InnerItem), Error = io::Error>,
        T: Sink<SinkItem = (RequestId, InnerSink), SinkError = io::Error>
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
