use io::{Ready, Readiness};
use reactor::{self, Source};
use mio::{self, Evented, EventSet, SetReadiness, Poll, PollOpt, Token};
use futures::{Future, Task};
use std::io;
use std::cell::RefCell;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

/// Processes multiple Futures and return their completed values in order they
/// were pushed into the queue.
///
/// Currently, only one `Future` is polled at a time, but it should be possible
/// to configure the number of futures concurrently polled, which would
/// increase parallelism.
pub struct AwaitQueue<T: Future> {
    // TODO: Make this better
    next_val: Arc<Mutex<Option<Result<T::Item, T::Error>>>>,
    val: Option<Result<T::Item, T::Error>>,
    task: Option<Task>,
    remaining: VecDeque<T>,
    in_flight: bool,
    source: Source,
    registration: Registration,
}

// TODO: Extract
struct Registration {
    inner: RefCell<Option<(mio::Registration, SetReadiness)>>,
}

impl<T> AwaitQueue<T>
    where T: Future
{
    /// Create an `AwaitQueue` with an initial capacity of `n`
    pub fn with_capacity(n: usize) -> io::Result<AwaitQueue<T>> {
        let registration = Registration {
            inner: RefCell::new(None),
        };

        let source = try!(reactor::register_source(&registration, Ready::readable()));

        Ok(AwaitQueue {
            next_val: Arc::new(Mutex::new(None)),
            val: None,
            task: None,
            remaining: VecDeque::with_capacity(n),
            in_flight: false,
            source: source,
            registration: registration,
        })
    }

    /// Return the number of queued futures.
    pub fn len(&self) -> usize {
        let mut len = self.remaining.len();

        if self.in_flight {
            len += 1;
        }

        len
    }

    /// Returns true if there are no queued futures.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Push a new future for processing.
    pub fn push(&mut self, future: T) {
        trace!("AwaitQueue::push");
        if self.in_flight {
            self.remaining.push_back(future);
            return;
        }

        self.schedule_future(future, true);
    }

    /// Push a new future for processing, attempting to resolve immediately
    ///
    /// Returns the resolved future if the queue is empty and the future is
    /// ready
    pub fn push_poll(&mut self, future: T) -> Option<Result<T::Item, T::Error>> {
        trace!("AwaitQueue::push_poll");
        if self.in_flight {
            self.remaining.push_back(future);
            return None;
        }

        if self.schedule_future(future, false) {
            // the value can be read from immediately and no further work is
            // needed
            self.in_flight = false;
            self.val.take()
        } else {
            None
        }
    }

    /// Poll for the next completed value.
    ///
    /// If the future at the head of the `AwaitQueue` is complete, the result
    /// will be returned and the next future will begin being processed. If the
    /// future at the head of the queue is not ready, `None` is returned.
    pub fn poll(&mut self) -> Option<Result<T::Item, T::Error>> {
        trace!("AwaitQueue::poll");

        if !self.is_readable() {
            return None;
        }

        let v = self.val.take().or_else(|| {
            self.next_val.lock().unwrap().take()
        });

        // The queue is not going to be readable anymore
        self.source.unset_readable();

        if let Some(v) = v {
            trace!("AwaitQueue::poll;      -> Got value");
            // No futures are in flight
            self.in_flight = false;

            // Unset the `Evented` readiness
            self.registration.set_readiness().unwrap()
                .set_readiness(EventSet::none()).unwrap();

            // Track progress at the source
            self.source.advance();

            // Schedule the next future
            self.schedule_next();

            // Return the value
            return Some(v);
        }

        None
    }

    fn schedule_next(&mut self) {
        if let Some(future) = self.remaining.pop_front() {
            self.schedule_future(future, true);
        }
    }

    fn schedule_future(&mut self, mut f: T, notify: bool) -> bool {
        use futures::Poll;

        trace!("AwaitQueue::schedule_future");

        let mut task = self.task.take().unwrap_or_else(Task::new);

        self.in_flight = true;

        match f.poll(&mut task) {
            Poll::Ok(v) => {
                trace!("AwaitQueue::schedule_future         -> done immediately");
                self.task = Some(task);
                self.val = Some(Ok(v));

                if notify {
                    self.registration.set_readiness().unwrap().set_readiness(EventSet::readable()).unwrap();
                }

                true
            }
            Poll::Err(e) => {
                self.task = Some(task);
                self.val = Some(Err(e));

                if notify {
                    self.registration.set_readiness().unwrap().set_readiness(EventSet::readable()).unwrap();
                }

                true
            }
            Poll::NotReady => {
                let set_readiness = self.registration.set_readiness().unwrap();
                let dst = self.next_val.clone();

                let f = f.then(move |res| {
                    trace!("          future received value");
                    *dst.lock().unwrap() = Some(res);
                    set_readiness.set_readiness(EventSet::readable()).unwrap();
                    Ok(())
                });

                task.run(Box::new(f));
                false
            }
        }
    }
}

impl<T: Future> Readiness for AwaitQueue<T> {
    fn is_readable(&self) -> bool {
        self.val.is_some() || self.source.is_readable()
    }

    fn is_writable(&self) -> bool {
        false
    }
}

impl Registration {
    fn set_readiness(&self) -> Option<SetReadiness> {
        let inner = self.inner.borrow();

        match *inner {
            Some((_, ref set_readiness)) => Some(set_readiness.clone()),
            _ => None,
        }
    }
}

impl Evented for Registration {
    fn register(&self, poll: &Poll, token: Token, interest: EventSet, opts: PollOpt) -> io::Result<()> {
        let mut inner = self.inner.borrow_mut();

        if inner.is_some() {
            return Err(io::Error::new(io::ErrorKind::Other, "already registered"));
        }

        let mio = mio::Registration::new(poll, token, interest, opts);
        *inner = Some(mio);
        Ok(())
    }

    fn reregister(&self, poll: &Poll, token: Token, interest: EventSet, opts: PollOpt) -> io::Result<()> {
        let inner = self.inner.borrow();

        match *inner {
            Some((ref r, _)) => r.update(poll, token, interest, opts),
            _ => Err(io::Error::new(io::ErrorKind::Other, "not registered")),
        }
    }

    fn deregister(&self, poll: &Poll) -> io::Result<()> {
        let inner = self.inner.borrow();

        match *inner {
            Some((ref r, _)) => r.deregister(poll),
            _ => Err(io::Error::new(io::ErrorKind::Other, "not registered")),
        }
    }
}
