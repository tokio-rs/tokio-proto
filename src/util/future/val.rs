//! A concrete implementation of `futures::Future`. It is similar in spirit as
//! `futures::Promise`, but is better suited for use with Tokio.

use super::{Cancellation, SyncFuture, Callback};
use futures::{Future, Poll, Task};
use std::mem;
use std::cell::Cell;
use std::sync::{Arc, Mutex};
use self::State::*;

/// A future representing the completion of an asynchronous computation.
///
/// This is created by the `pair` function.
pub struct Val<T, E> {
    inner: Arc<Inner<T, E>>,
}

/// The `Complete` half of `Val` used to send the result of an asynchronous
/// computation to the consumer of `Val`.
///
/// This is created by the `pair` function.
pub struct Complete<T, E> {
    inner: Arc<Inner<T, E>>,
    cancellation: Cell<bool>,
}

// Currently implemented with a mutex, but this is only to get something
// working. This should be rewritten to use a lock free strategy.
struct Inner<T, E> {
    state: Mutex<State<T, E>>,
}

enum State<T, E> {
    Init {
        consumer: Option<Callback>,
        cancellation: Option<Callback>
    },
    Completed(Option<Result<T, E>>),
    Cancelled,
    Consumed,
}

/// Create and return a new `Complete` / `Val` pair.
///
/// `Complete` is used to send the result of an asynchronous computation to the
/// consumer of `Val`.
pub fn pair<T, E>() -> (Complete<T, E>, Val<T, E>) {
    let inner = Arc::new(Inner {
        state: Mutex::new(State::Init {
            consumer: None,
            cancellation: None,
        }),
    });

    let tx = Complete {
        inner: inner.clone(),
        cancellation: Cell::new(false),
    };
    let rx = Val {
        inner: inner,
    };

    (tx, rx)
}

/*
 *
 * ===== Val =====
 *
 */

impl<T, E> Future for Val<T, E>
    where T: Send + 'static,
          E: Send + 'static,
{
    type Item = T;
    type Error = E;

    fn poll(&mut self, task: &mut Task) -> Poll<T, E> {
        self.inner.poll(task)
    }

    fn schedule(&mut self, task: &mut Task) {
        self.inner.schedule(task)
    }
}

impl<T, E> Drop for Val<T, E> {
    fn drop(&mut self) {
        self.inner.cancel();
    }
}

/*
 *
 * ===== Complete =====
 *
 */

impl<T, E> Complete<T, E>
    where T: Send + 'static,
          E: Send + 'static,
{
    /// Successfully complete the associated `Val` with the given value.
    pub fn complete(self, val: T) {
        self.inner.complete(Some(Ok(val)), true);
    }

    /// Complete the associated `Val` with the given error
    pub fn error(self, err: E) {
        self.inner.complete(Some(Err(err)), true);
    }

    /// Abort the computation. This will cause the associated `Val` to panic on
    /// a call to `poll`.
    pub fn abort(self) {
        self.inner.complete(None, true);
    }

    /// Returns a `Future` representing the consuming end cancelling interest
    /// in the future.
    ///
    /// This function can only be called once.
    ///
    /// # Panics
    ///
    /// A second call to this function will result in a panic.
    pub fn cancellation(&self) -> Cancellation {
        if self.cancellation.get() {
            panic!("cancellation future already obtained");
        }

        self.cancellation.set(true);
        Cancellation::new(self.inner.clone())
    }
}

impl<T, E> Drop for Complete<T, E> {
    fn drop(&mut self) {
        self.inner.complete(None, false);
    }
}

/*
 *
 * ===== Inner =====
 *
 */

impl<T, E> Inner<T, E> {
    /// Complete the future with the given result
    fn complete(&self, res: Option<Result<T, E>>, panic: bool) {
        let mut state = match self.state.lock() {
            Ok(s) => s,
            Err(_) => {
                if panic { panic!("failed to lock mutex") };
                return;
            }
        };

        let cb;

        match state.take() {
            Init { consumer, .. } => cb = consumer,
            s => {
                if res.is_some() {
                    drop(state);
                    panic!("attempting to complete already completed future");
                } else {
                    *state = s;
                    return;
                }
            }
        }

        *state = Completed(res);
        drop(state);

        if let Some(cb) = cb {
            cb.call_box(); // Invoke callback
        }
    }

    /// Cancel interest in the future
    fn cancel(&self) {
        let mut state = match self.state.lock() {
            Ok(s) => s,
            Err(_) => return,
        };

        let cb;

        match state.take() {
            Init { cancellation, .. } => cb = cancellation,
            Completed(_) | Cancelled | Consumed => {
                return; // Cannot cancel from these states
            }
        }

        *state = Cancelled;
        drop(state);

        if let Some(cb) = cb {
            cb.call_box(); // Invoke callback
        }
    }

    /// Poll the inner state for a value
    fn poll(&self, _: &mut Task) -> Poll<T, E> {
        let mut state = self.state.lock().unwrap();

        if state.is_complete() {
            match state.take() {
                Completed(Some(Ok(v))) => Poll::Ok(v),
                Completed(Some(Err(e))) => Poll::Err(e),
                Completed(None) => panic!("Complete dropped without producing a value"),
                Consumed => {
                    drop(state);
                    panic!("Val already consumed");
                }
                _ => unreachable!(),
            }
        } else {
            Poll::NotReady
        }
    }

    /// Associate completion with the given task
    fn schedule(&self, task: &mut Task) {
        let mut state = self.state.lock().unwrap();

        if state.in_flight() {
            let handle = task.handle().clone();
            state.set_consumer_cb(Box::new(move || handle.notify()));
        } else {
            task.handle().notify();
        }
    }
}

impl<T, E> SyncFuture for Inner<T, E>
    where T: Send + 'static,
          E: Send + 'static,
{
    fn poll(&self, _: &mut Task) -> Poll<bool, ()> {
        let state = self.state.lock().unwrap();

        match *state {
            Init { .. } => Poll::NotReady,
            Cancelled => Poll::Ok(true),
            _ => Poll::Ok(false),
        }
    }

    fn schedule(&self, task: &mut Task) {
        let mut state = self.state.lock().unwrap();

        if state.in_flight() {
            let handle = task.handle().clone();
            state.set_cancellation_cb(Box::new(move || handle.notify()));
        } else {
            drop(state);
            task.handle().notify();
        }
    }
}

impl<T, E> State<T, E> {
    fn in_flight(&self) -> bool {
        match *self {
            Init { .. } => true,
            _ => false,
        }
    }

    /// Returns true if in a completed state.
    fn is_complete(&self) -> bool {
        match *self {
            Completed(_) | Consumed => true,
             _ => false,
        }
    }

    fn set_consumer_cb(&mut self, cb: Callback) {
        match *self {
            Init { ref mut consumer, .. } => *consumer = Some(cb),
            _ => panic!("unexpected state"),
        }
    }

    fn set_cancellation_cb(&mut self, cb: Callback) {
        match *self {
            Init { ref mut cancellation, .. } => *cancellation = Some(cb),
            _ => panic!("unexpected state"),
        }
    }

    /// Sets the current state to Consumed and returns the original value
    fn take(&mut self) -> State<T, E> {
        mem::replace(self, State::Consumed)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use futures::Future;
    use std::sync::mpsc;

    #[test]
    fn test_complete_after_listen() {
        let (c, val) = pair::<u32, ()>();
        let (tx, rx) = mpsc::channel();

        val.then(move |res| {
            tx.send(res.unwrap()).unwrap();
            res
        }).forget();

        c.complete(123);

        assert_eq!(123, rx.recv().unwrap());
    }

    #[test]
    fn test_complete_before_listen() {
        let (c, val) = pair::<u32, ()>();
        let (tx, rx) = mpsc::channel();

        c.complete(123);

        val.then(move |res| {
            tx.send(res.unwrap()).unwrap();
            res
        }).forget();

        assert_eq!(123, rx.recv().unwrap());
    }

    #[test]
    #[should_panic(expected = "Complete dropped without producing a value")]
    fn test_polling_aborted_future_panics() {
        let (c, val) = pair::<u32, ()>();
        val.then(move |res| {
            println!("WAT: {:?}", res);
            res
        }).forget();

        c.abort();
    }

    #[test]
    fn test_cancellation_future() {
        let (c, val) = pair::<u32, ()>();
        let (tx, rx) = mpsc::channel();

        c.cancellation().then(move |res| {
            tx.send(123).unwrap();
            res
        }).forget();

        assert!(rx.try_recv().is_err());

        drop(val);
        assert_eq!(123, rx.recv().unwrap());
    }
}
