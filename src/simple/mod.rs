pub mod pipeline;
pub mod multiplex;


use std::marker::PhantomData;
use futures::{Future, Poll, Async};
use std::io;

// A utility struct to enable "lifting" from an RPC to a streaming proto, which
// is how RPC protos are implemented under the hood. Unfortunately:
//
// - Having a blanket impl that lifts *directly* from
//   e.g. rpc::pipeline::ServerProto to streaming::pipeline::ServerProto causes
//   inference problems, so we need a newtype.
//
// - We can't do `LiftProto<'a, P: 'a>(&'a P)` because of the `'static` requirement,
//   but we need to work with references because binding a transport taked `&self`.
//
// Thus, we use a newtype over the actual protocol type, which requires a bit of
// transmute hackery to transform references. Since newtypes are guaranteed not
// to change layout, this is kosher.
struct LiftProto<P>(P);

impl<P> LiftProto<P> {
    fn from_ref(proto: &P) -> &LiftProto<P> {
        unsafe { ::std::mem::transmute(proto) }
    }

    fn lower(&self) -> &P {
        &self.0
    }
}

trait FromTransport<T> {
    fn from_transport(T) -> Self;
}

// Lifts the Bind from the underlying transport
struct LiftBind<F, A> {
    fut: F,
    marker: PhantomData<A>,
}

impl<F, A> LiftBind<F, A> {
    pub fn lift(f: F) -> LiftBind<F, A> {
        LiftBind {
            fut: f,
            marker: PhantomData,
        }
    }
}

impl<F, A> Future for LiftBind<F, A> where
    F: Future<Error = io::Error>,
    A: FromTransport<F::Item>,
{
    type Item = A;
    type Error = io::Error;

    fn poll(&mut self) -> Poll<Self::Item, io::Error> {
        Ok(Async::Ready(A::from_transport(try_ready!(self.fut.poll()))))
    }
}
