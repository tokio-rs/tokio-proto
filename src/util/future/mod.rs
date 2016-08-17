//! Utilities useful for working with Tokio and futures.
//!
//! # Waiting
//!
//! Anything that would require waiting for a value on a `Task` needs to be
//! Tokio aware, including waiting for futures to complete. `Await` (to be
//! written) and `AwaitQueue` provide facilities for notifying the Reactor when
//! a future completes and may be polled.

mod await;
mod channel;
mod queue;
mod val;

pub use self::channel::{channel, Sender, BusySender, Receiver};
pub use self::val::{pair, Complete, Val};

#[doc(hidden)]
pub use self::queue::AwaitQueue;
#[doc(hidden)]
pub use self::await::{Await, AwaitStream};

use futures::{Future, Poll, Task};
use std::sync::Arc;

/// A future representing the cancellation in interest by the consumer of
/// `Val`.
///
/// If a `Val` is dropped without ever attempting to read the value, then it
/// becomes impossible to ever receive the result of the underlying
/// computation. This indicates that there is no interest in the computation
/// and it may be cancelled.
///
/// In this case, this future will be completed. The asynchronous computation
/// is able to listen for this completion and abort work early.
pub struct Cancellation {
    inner: Arc<SyncFuture>,
}

// A little hacky, but implementing Future on Inner allows losing the generics
// on Cancellation
trait SyncFuture: Send + Sync + 'static {
    fn poll(&self, task: &mut Task) -> Poll<bool, ()>;

    fn schedule(&self, task: &mut Task);
}

trait FnBox: Send + 'static {
    fn call_box(self: Box<Self>);
}

type Callback = Box<FnBox>;

impl Cancellation {
    fn new(inner: Arc<SyncFuture>) -> Cancellation {
        Cancellation { inner: inner }
    }
}

impl Future for Cancellation {
    type Item = bool;
    type Error = ();

    fn poll(&mut self, task: &mut Task) -> Poll<bool, ()> {
        self.inner.poll(task)
    }

    fn schedule(&mut self, task: &mut Task) {
        self.inner.schedule(task)
    }
}

impl<F> FnBox for F
    where F: FnOnce() + Send + 'static
{
    fn call_box(self: Box<F>) {
        (*self)()
    }
}
