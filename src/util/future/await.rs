use io::{Ready, Readiness};
use reactor::{self, Source};
use mio::{self, Evented, EventSet, SetReadiness, Poll, PollOpt, Token};
use futures::{self, Future, Task};
use futures::stream::Stream;
use std::io;
use std::cell::RefCell;
use std::sync::{Arc, Mutex};

// TODO: This is terrible and should go away

/// Wait on a given future to complete on a Tokio task
pub struct Await<T: Future> {
    val: Arc<Mutex<Option<Result<T::Item, T::Error>>>>,
    source: Source,
    registration: Registration,
}

/// Wait on a given stream to complete the next value
pub struct AwaitStream<T: Stream> {
    val: Arc<Mutex<Option<Result<Option<T::Item>, T::Error>>>>,
    task: futures::TaskHandle,
    source: Source,
    registration: Registration,
}

struct Pump<T: Stream> {
    stream: T,
    val: Arc<Mutex<Option<Result<Option<T::Item>, T::Error>>>>,
    set_readiness: SetReadiness,
}

struct Registration {
    inner: RefCell<Option<(mio::Registration, SetReadiness)>>,
}

impl<T> Await<T> where T: Future {
    /// Create a new await for the given future
    pub fn new(f: T) -> io::Result<Await<T>> {
        let registration = Registration {
            inner: RefCell::new(None),
        };

        let source = try!(reactor::register_source(&registration, Ready::readable()));
        let set_readiness = registration.set_readiness().unwrap();
        let dst = Arc::new(Mutex::new(None));
        let task = Task::new();

        let await = Await {
            val: dst.clone(),
            source: source,
            registration: registration,
        };

        let f = f.then(move |res| {
            trace!("future received value");
            *dst.lock().unwrap() = Some(res);
            set_readiness.set_readiness(EventSet::readable()).unwrap();
            Ok(())
        });

        task.run(Box::new(f));

        Ok(await)
    }

    /// Poll for the completed value.
    pub fn poll(&mut self) -> Option<Result<T::Item, T::Error>> {
        trace!("Await::poll");

        if !self.is_readable() {
            return None;
        }

        let v = self.val.lock().unwrap().take();

        // The queue is not going to be readable anymore
        self.source.unset_readable();

        if let Some(v) = v {
            trace!("Await::poll; Got value");

            // Unset the `Evented` readiness
            self.registration.set_readiness().unwrap()
                .set_readiness(EventSet::none()).unwrap();

            // Track progress at the source
            self.source.advance();

            // Return the value
            return Some(v);
        }

        None
    }
}

impl<T: Stream> AwaitStream<T> {
    /// Create a new await for the given future
    pub fn new(stream: T) -> io::Result<AwaitStream<T>> {
        let registration = Registration {
            inner: RefCell::new(None),
        };

        let source = try!(reactor::register_source(&registration, Ready::readable()));
        let set_readiness = registration.set_readiness().unwrap();
        let dst = Arc::new(Mutex::new(None));
        let task = Task::new();

        let await = AwaitStream {
            val: dst.clone(),
            task: task.handle().clone(),
            source: source,
            registration: registration,
        };

        let pump = Pump {
            stream: stream,
            val: dst,
            set_readiness: set_readiness,
        };

        task.run(Box::new(pump));

        Ok(await)
    }

    /// Poll for the completed value.
    pub fn poll(&mut self) -> Option<Result<Option<T::Item>, T::Error>> {
        trace!("AwaitStream::poll");

        if !self.is_readable() {
            return None;
        }

        let v = self.val.lock().unwrap().take();

        // The queue is not going to be readable anymore
        self.source.unset_readable();

        if let Some(v) = v {
            trace!("Await::poll; Got value");

            // Unset the `Evented` readiness
            self.registration.set_readiness().unwrap()
                .set_readiness(EventSet::none()).unwrap();

            // Track progress at the source
            self.source.advance();

            // Notify pump there is more work to be done
            self.task.notify();

            // Return the value
            return Some(v);
        }

        None
    }
}

impl<T: Future> Readiness for Await<T> {
    fn is_readable(&self) -> bool {
        let v = self.val.lock().unwrap().is_some();
        v || self.source.is_readable()
    }

    fn is_writable(&self) -> bool {
        false
    }
}

impl<T: Stream> Future for Pump<T> {
    type Item = ();
    type Error = ();

    fn poll(&mut self, task: &mut Task) -> futures::Poll<(), ()> {
        use futures::Poll;

        let mut state = self.val.lock().unwrap();
        let last;

        if state.is_some() {
            return Poll::NotReady;
        }

        match self.stream.poll(task) {
            Poll::NotReady => return Poll::NotReady,
            Poll::Ok(v) => {
                last = v.is_none();
                *state = Some(Ok(v));
            }
            Poll::Err(e) => {
                last = true;
                *state = Some(Err(e));
            }
        }

        self.set_readiness.set_readiness(EventSet::readable()).unwrap();

        if last {
            Poll::Ok(())
        } else {
            Poll::NotReady
        }
    }

    fn schedule(&mut self, task: &mut Task) {
        let state = self.val.lock().unwrap();

        if state.is_none() {
            self.stream.schedule(task);
        }
    }
}

impl<T: Stream> Readiness for AwaitStream<T> {
    fn is_readable(&self) -> bool {
        let v = self.val.lock().unwrap().is_some();
        v || self.source.is_readable()
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
