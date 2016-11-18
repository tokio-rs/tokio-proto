//! Pipelined RPC protocols.
//!
//! See the crate-level docs for an overview.

mod client;
pub use self::client::ClientProto;

mod server;
pub use self::server::ServerProto;

/// A marker used to flag protocols as being pipelined RPC.
///
/// This is an implementation detail; to actually implement a protocol,
/// implement the `ClientProto` or `ServerProto` traits in this module.
pub struct Pipeline;

// This is a submodule so that `LiftTransport` can be marked `pub`, to satisfy
// the no-private-in-public checker.
mod lift {
    use std::io;
    use std::marker::PhantomData;

    use streaming::pipeline::{Frame, StreamingTransport};
    use transport::Transport;
    use futures::{Future, StartSend, Poll, Async, AsyncSink};

    // Lifts an implementation of RPC-style transport to streaming-style transport
    pub struct LiftTransport<T, E>(pub T, pub PhantomData<E>);

    // Lifts the Bind from the underlying transport
    pub struct LiftBind<A, F, E> {
        fut: F,
        marker: PhantomData<(A, E)>,
    }

    impl<A: 'static, T: Transport<A>, E: 'static> Transport<A> for LiftTransport<T, E> {
        type ReadFrame = Frame<T::ReadFrame, (), E>;
        type WriteFrame = Frame<T::WriteFrame, (), E>;
        type Bind = LiftBind<A, T::Bind, E>;

        fn bind(io: A) -> LiftBind<A, T::Bind, E> {
            LiftBind::lift(T::bind(io))
        }

        fn tick(&mut self) {
            self.0.tick()
        }

        fn poll(&mut self) -> Poll<Option<Self::ReadFrame>, io::Error> {
            let item = try_ready!(self.0.poll());
            Ok(item.map(|msg| {
                Frame::Message { message: msg, body: false }
            }).into())
        }

        fn start_send(&mut self, request: Self::WriteFrame)
                      -> StartSend<Self::WriteFrame, io::Error> {
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
    }

    impl<A: 'static, T: Transport<A>, E: 'static> StreamingTransport<A> for LiftTransport<T, E> {}

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
