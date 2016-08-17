use super::{Cancellation, Callback, SyncFuture};
use futures::{Future, Poll, Task};
use futures::stream::{Stream};
use std::mem;
use std::cell::Cell;
use std::sync::{Arc, Mutex};
use self::State::*;

/// The transmission end of a channel which is used to send values.
///
/// This is created by the `channel` method in the `stream` module.
pub struct Sender<T, E>
    where T: Send + 'static,
          E: Send + 'static,
{
    inner: Arc<Inner<T, E>>,
    cancellation: Cell<bool>,
}

/// A future returned by the `Sender::send` method which will resolve to the
/// sender once it's available to send another message.
pub struct BusySender<T, E>
    where T: Send + 'static,
          E: Send + 'static,
{
    sender: Option<Sender<T, E>>,
}

/// The receiving end of a channel which implements the `Stream` trait.
///
/// This is a concrete implementation of a stream which can be used to represent
/// a stream of values being computed elsewhere. This is created by the
/// `channel` method in the `stream` module.
pub struct Receiver<T, E>
    where T: Send + 'static,
          E: Send + 'static,
{
    inner: Arc<Inner<T, E>>,
}

/// A specialized `Result` for sending a value over a channel.
pub type SendResult<T, E> = Result<BusySender<T, E>, SendError<T, E>>;

/// An error returned from the `send` function on channels.
///
/// A `send` operation can only fail if the receiving end of a channel is
/// disconnected, implying that the data could never be received. The error
/// contains the data being sent as a payload so it can be recovered.
#[derive(Debug)]
pub struct SendError<T, E>(pub Result<T, E>);

/// This only happens if the receiving end dropped
#[derive(Debug)]
pub struct BusySendError;

// Currently implemented with a mutex, but this is only to get something
// working. This should be rewritten to use a lock free strategy.
struct Inner<T, E> {
    state: Mutex<State<T, E>>,
}

enum State<T, E> {
    Init {
        consumer: Option<Callback>,
        cancellation: Option<Callback>,
    },
    Value {
        val: Result<T, E>,
        producer: Option<Callback>,
        cancellation: Option<Callback>,
    },
    Final {
        val: Result<T, E>,
    },
    Cancelled,
    Consumed,
}

/// Creates an in-memory channel implementation of the `Stream` trait.
///
/// This method creates a concrete implementation of the `Stream` trait which
/// can be used to send values across threads in a streaming fashion. This
/// channel is unique in that it implements back pressure to ensure that the
/// sender never outpaces the receiver. The `Sender::send` method will only
/// allow sending one message and the next message can only be sent once the
/// first was consumed.
///
/// The `Receiver` returned implements the `Stream` trait and has access to any
/// number of the associated combinators for transforming the result.
pub fn channel<T, E>() -> (Sender<T, E>, Receiver<T, E>)
    where T: Send + 'static,
          E: Send + 'static,
{
    let inner = Arc::new(Inner {
        state: Mutex::new(State::Init {
            consumer: None,
            cancellation: None,
        }),
    });

    let tx = Sender {
        inner: inner.clone(),
        cancellation: Cell::new(false),
    };
    let rx = Receiver {
        inner: inner,
    };

    (tx, rx)
}

/*
 *
 * ===== Receiver =====
 *
 */

impl<T, E> Receiver<T, E>
    where T: Send + 'static,
          E: Send + 'static,
{
    /// Drops the `Receiver` only if the channel is empty.
    ///
    /// This function allows safely dropping the receiving end without losing
    /// any messages in the channel.
    pub fn try_release(self) -> Result<(), Self> {
        {
            let mut state = self.inner.state.lock().unwrap();

            match *state {
                Value { .. } | Final { .. } => {
                    // Fall through to the error case. This makes the borrow
                    // checker happy.
                },
                _ => {
                    let cb;

                    match state.take() {
                        Init { cancellation, .. } => cb = cancellation,
                        _ => return Ok(()),
                    }

                    *state = Cancelled;
                    drop(state);

                    if let Some(cb) = cb {
                        cb.call_box(); // Invoke callback
                    }

                    return Ok(());
                }
            }
        };

        Err(self)
    }
}

impl<T, E> Stream for Receiver<T, E>
    where T: Send + 'static,
          E: Send + 'static,
{
    type Item = T;
    type Error = E;

    fn poll(&mut self, task: &mut Task) -> Poll<Option<T>, E> {
        self.inner.poll(task)
    }

    fn schedule(&mut self, task: &mut Task) {
        self.inner.schedule(task)
    }
}

impl<T, E> Drop for Receiver<T, E>
    where T: Send + 'static,
          E: Send + 'static,
{
    fn drop(&mut self) {
        self.inner.cancel();
    }
}

/*
 *
 * ===== Sender =====
 *
 */

impl<T, E> Sender<T, E>
    where T: Send + 'static,
          E: Send + 'static,
{
    /// Sends a new value along this channel to the receiver.
    ///
    /// This method consumes the sender and returns a future which will resolve
    /// to the sender again when the value sent has been consumed.
    pub fn send(self, val: Result<T, E>) -> SendResult<T, E> {
        match self.inner.send(val, true) {
            Ok(_) => Ok(BusySender { sender: Some(self) }),
            Err(val) => Err(SendError(val)),
        }
    }

    /// Returns a `Future` representing the consuming end cancelling interest
    /// in the stream.
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

impl<T, E> Drop for Sender<T, E>
    where T: Send + 'static,
          E: Send + 'static,
{
    fn drop(&mut self) {
        self.inner.close();
    }
}

/*
 *
 * ===== BusySender =====
 *
 */

impl<T, E> Future for BusySender<T, E>
    where T: Send + 'static,
          E: Send + 'static,
{
    type Item = Sender<T, E>;
    type Error = BusySendError;

    fn poll(&mut self, _task: &mut Task) -> Poll<Self::Item, Self::Error> {
        {
            // Sender
            let sender = self.sender.as_ref()
                .expect("BusySender already consumed");

            let state = sender.inner.state.lock().unwrap();

            match *state {
                // Fall through
                Init { .. } => {}
                Cancelled => {
                    return Poll::Err(BusySendError);
                }
                _ => {
                    drop(state);
                    panic!("unexpected state");
                }
            }
        }

        Poll::Ok(self.sender.take().expect("BusySender already consumed"))
    }

    fn schedule(&mut self, task: &mut Task) {
        if let Some(sender) = self.sender.as_ref() {
            let mut state = sender.inner.state.lock().unwrap();

            if state.is_value() {
                let handle = task.handle().clone();
                state.set_producer_cb(Box::new(move || handle.notify()));
            } else {
                task.handle().notify();
            }
        }
    }
}

/*
 *
 * ===== Inner =====
 *
 */

impl<T, E> Inner<T, E> {
    fn send(&self, res: Result<T, E>, panic: bool) -> Result<(), Result<T, E>>{
        let mut state = match self.state.lock() {
            Ok(s) => s,
            Err(_) => {
                if panic { panic!("failed to lock mutex") };
                return Err(res);
            }
        };

        let cb_consumer;
        let cb_cancellation;

        match state.take() {
            Init { consumer, cancellation } => {
                cb_consumer = consumer;
                cb_cancellation = cancellation;
            }
            Cancelled => {
                return Err(res);
            }
            _ => panic!("unexpected state"),
        }

        *state = Value {
            val: res,
            producer: None,
            cancellation: cb_cancellation,
        };

        drop(state);

        if let Some(cb) = cb_consumer {
            cb.call_box();
        }

        Ok(())
    }

    fn close(&self) {
        let mut state = match self.state.lock() {
            Ok(s) => s,
            Err(_) => return,
        };

        let cb;
        let next;

        match state.take() {
            Init { consumer, .. } => {
                cb = consumer;
                next = Consumed;
            }
            Value { val, .. } => {
                cb = None;
                next = Final { val: val };
            }
            Final { .. } | Cancelled | Consumed => {
                return; // Cannot cancel from these states
            }
        }

        *state = next;
        drop(state);

        if let Some(cb) = cb {
            cb.call_box(); // Invoke callback
        }
    }

    fn cancel(&self) {
        let mut state = match self.state.lock() {
            Ok(s) => s,
            Err(_) => return,
        };

        let cb;

        match state.take() {
            Init { cancellation, .. } => cb = cancellation,
            Value { cancellation, .. } => cb = cancellation,
            Final { .. } | Cancelled | Consumed => {
                return; // Cannot cancel from these states
            }
        }

        *state = Cancelled;
        drop(state);

        if let Some(cb) = cb {
            cb.call_box(); // Invoke callback
        }
    }

    fn poll(&self, _: &mut Task) -> Poll<Option<T>, E> {
        let mut state = self.state.lock().unwrap();

        if state.is_complete() {
            let cb;
            let next;

            // When completed with Some(Err(..)) or None, it is not possible
            // for there to be a producer callback. Also, the state will
            // transition to consume

            let res = match state.take() {
                Value { val: v, producer, cancellation } => {
                    next = Init {
                        consumer: None,
                        cancellation: cancellation,
                    };

                    cb = producer;

                    match v {
                        Ok(v) => Poll::Ok(Some(v)),
                        Err(e) => Poll::Err(e),
                    }
                }
                Final { val: v } => {
                    next = Consumed;
                    cb = None;

                    match v {
                        Ok(v) => Poll::Ok(Some(v)),
                        Err(e) => Poll::Err(e),
                    }
                }
                Consumed => {
                    return Poll::Ok(None);
                }
                _ => unreachable!(),
            };

            *state = next;
            drop(state);

            if let Some(cb) = cb {
                cb.call_box();
            }

            res
        } else {
            Poll::NotReady
        }
    }

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
            Init { .. } | Value { .. } => Poll::NotReady,
            Cancelled => Poll::Ok(true),
            _ => Poll::Ok(false),
        }
    }

    fn schedule(&self, task: &mut Task) {
        let mut state = self.state.lock().unwrap();

        match *state {
            Init { ref mut cancellation, .. } |
            Value { ref mut cancellation, .. } => {
                let handle = task.handle().clone();
                *cancellation = Some(Box::new(move || handle.notify()));
                return;
            },
            _ => {}
        }

        drop(state);
        task.handle().notify();
    }
}

/*
 *
 * ===== State =====
 *
 */

impl<T, E> State<T, E> {
    fn in_flight(&self) -> bool {
        match *self {
            Init { .. } => true,
            _ => false,
        }
    }

    fn is_value(&self) -> bool {
        match *self {
            Value { .. } => true,
            _ => false,
        }
    }

    fn is_complete(&self) -> bool {
        match *self {
            Value { .. } | Final { .. } | Consumed => true,
            _ => false,
        }
    }

    fn set_consumer_cb(&mut self, cb: Callback) {
        match *self {
            Init { ref mut consumer, .. } => *consumer = Some(cb),
            _ => panic!("unexpected state"),
        }
    }

    fn set_producer_cb(&mut self, cb: Callback) {
        match *self {
            Value { ref mut producer, .. } => *producer = Some(cb),
            _ => panic!("unexpected state"),
        }
    }

    fn take(&mut self) -> State<T, E> {
        mem::replace(self, State::Consumed)
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use futures::{self, Future, Poll, Task};
    use futures::stream::Stream;
    use std::sync::mpsc;

    #[test]
    fn test_send_value_after_listen() {
        let (tx1, rx1) = channel::<u32, ()>();
        let (tx2, rx2) = mpsc::channel();

        rx1.for_each(move |res| {
            tx2.send(res).unwrap();
            Ok(())
        }).forget();

        let tx1 = tx1.send(Ok(123));
        drop(tx1);

        assert_eq!(123, rx2.recv().unwrap());
        assert!(rx2.recv().is_err());
    }

    #[test]
    fn test_send_value_before_listen() {
        let (tx1, rx1) = channel::<u32, ()>();
        let (tx2, rx2) = mpsc::channel();

        let tx1 = tx1.send(Ok(123));
        drop(tx1);

        rx1.for_each(move |res| {
            tx2.send(res).unwrap();
            Ok(())
        }).forget();

        assert_eq!(123, rx2.recv().unwrap());
        assert!(rx2.recv().is_err());
    }

    #[test]
    fn test_send_multiple_values() {
        let (tx1, rx1) = channel::<u32, ()>();
        let (tx2, rx2) = mpsc::channel();

        rx1.for_each(move |res| {
            tx2.send(res).unwrap();
            Ok(())
        }).forget();

        let mut tx1 = tx1.send(Ok(123)).unwrap();

        assert_eq!(123, rx2.recv().unwrap());

        let mut task = Task::new();

        let tx1 = match tx1.poll(&mut task) {
            Poll::Ok(tx) => tx,
            _ => panic!("unexpected result"),
        };

        tx1.send(Ok(456)).unwrap();

        assert_eq!(456, rx2.recv().unwrap());
        assert!(rx2.recv().is_err());
    }

    #[test]
    fn test_sending_error() {
        let (tx1, rx1) = channel::<u32, ()>();
        let (tx2, rx2) = mpsc::channel();

        rx1
            .for_each(|_| {
                unreachable!();
            })
            .or_else(move |err| {
                tx2.send(err).unwrap();
                Ok::<(), ()>(())
            })
            .forget();

        tx1.send(Err(())).unwrap();

        assert_eq!((), rx2.recv().unwrap());
    }

    #[test]
    #[ignore]
    fn test_immediately_taking_next_step_doesnt_stack_overflow() {
        // Blocked on https://github.com/alexcrichton/futures-rs/issues/62
        let (tx1, rx1) = channel::<u32, ()>();
        let (tx2, rx2) = mpsc::channel();

        fn count_down(i: u32, f: Sender<u32, ()>)
            -> Box<Future<Item = (), Error = ()>>
        {
            match f.send(Ok(i)) {
                Ok(busy) => {
                    if i > 0 {
                        busy
                            .map_err(|_| ())
                            .and_then(move |sender| {
                                count_down(i - 1, sender)
                            })
                            .boxed()
                    } else {
                        futures::finished::<(), ()>(()).boxed()
                    }
                }
                _ => panic!("nope"),
            }
        }

        rx1.for_each(move |v| {
            tx2.send(v).unwrap();
            Ok(())
        }).forget();

        count_down(50_000, tx1).forget();

        let cnt = rx2.iter().count();
        assert_eq!(50_000, cnt);
    }

    #[test]
    fn test_cancellation_future() {
        let (tx1, rx1) = channel::<u32, ()>();
        let (tx2, rx2) = mpsc::channel();

        tx1.cancellation().then(move |res| {
            tx2.send(123).unwrap();
            res
        }).forget();

        assert!(rx2.try_recv().is_err());

        drop(rx1);
        assert_eq!(123, rx2.recv().unwrap());
    }

    #[test]
    fn test_try_release() {
        let (tx, rx) = channel::<u32, ()>();

        let _busy = tx.send(Ok(123));

        let mut rx = match rx.try_release() {
            Ok(_) => panic!("should not happen"),
            Err(rx) => rx,
        };

        let mut task = Task::new();

        match rx.poll(&mut task) {
            Poll::Ok(Some(v)) => assert_eq!(123, v),
            _ => panic!("should not happen"),
        }

        assert!(rx.try_release().is_ok());
    }
}
