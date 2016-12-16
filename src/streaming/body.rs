use std::fmt;

use futures::{Async, Poll, Stream};
use futures::sync::mpsc;

/// Body stream
pub struct Body<T, E> {
    inner: Inner<T, E>,
}

enum Inner<T, E> {
    Once(Option<T>),
    Stream(mpsc::Receiver<Result<T, E>>),
    Empty,
}

impl<T, E> Body<T, E> {
    /// Return an empty body stream
    pub fn empty() -> Body<T, E> {
        Body { inner: Inner::Empty }
    }

    /// Return a body stream with an associated sender half
    pub fn pair() -> (mpsc::Sender<Result<T, E>>, Body<T, E>) {
        let (tx, rx) = mpsc::channel(0);
        let rx = Body { inner: Inner::Stream(rx) };
        (tx, rx)
    }
}

impl<T, E> Stream for Body<T, E> {
    type Item = T;
    type Error = E;

    fn poll(&mut self) -> Poll<Option<T>, E> {
        match self.inner {
            Inner::Once(ref mut val) => Ok(Async::Ready(val.take())),
            Inner::Stream(ref mut s) => {
                match s.poll().unwrap() {
                    Async::Ready(None) => Ok(Async::Ready(None)),
                    Async::Ready(Some(Ok(e))) => Ok(Async::Ready(Some(e))),
                    Async::Ready(Some(Err(e))) => Err(e),
                    Async::NotReady => Ok(Async::NotReady),
                }
            }
            Inner::Empty => Ok(Async::Ready(None)),
        }
    }
}

impl<T, E> From<mpsc::Receiver<Result<T, E>>> for Body<T, E> {
    fn from(src: mpsc::Receiver<Result<T, E>>) -> Body<T, E> {
        Body { inner: Inner::Stream(src) }
    }
}

impl<T, E> From<T> for Body<T, E> {
    fn from(val: T) -> Body<T, E> {
        Body { inner: Inner::Once(Some(val)) }
    }
}

impl<T, E> fmt::Debug for Body<T, E> {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        write!(fmt, "Body {{ [stream of values] }}")
    }
}
