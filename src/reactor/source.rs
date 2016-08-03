use io::Ready;
use reactor::reactor::{EventLoop, EventLoopId};
use mio::Token;
use std::cell::Cell;
use std::rc::Rc;

/// A `Source` represents any external input that a Task can use in order to
/// make forward progress.
///
/// The functions on `Source` interact with the `Reactor` in terms of
/// reading and registering interest on the `Source.
///
/// Using `Source` directly should only be required when implementing a Tokio
/// aware source. Tokio comes with a number of source implementations already,
/// see the `tcp` and `util` modules for examples.
pub struct Source {
    inner: Rc<Inner>,
}

/// Weak Source handle. This is kept by the reactor.
pub struct Weak {
    inner: Rc<Inner>,
}

struct Inner {
    // The current readiness state of the source
    readiness: Cell<Ready>,
    // The source's token
    token: Token,
    // Ref of the task that is associated with the source
    task: Cell<Option<Token>>,
    // The source's readiness interest
    interest: Cell<Ready>,
    // The tick_num at which interest was registered with this source
    interest_at: Cell<u64>,
    // Identifier of the EventLoop this source is associated with
    event_loop: EventLoopId,
}

impl Source {

    /// Returns true if the reactor believes that the source is currently
    /// readable.
    ///
    /// When the function returns `false`, read interest is registered with the
    /// reactor.
    pub fn is_readable(&self) -> bool {
        let is_readable = self.inner.readiness.get().is_readable();

        if !is_readable {
            self.interest(Ready::readable());
        }

        is_readable
    }

    /// Notify the reactor that the source is currently not readable
    pub fn unset_readable(&self) {
        self.interest(Ready::readable());
        let readiness = self.inner.readiness.get();
        self.inner.readiness.set(readiness - Ready::readable());
    }

    /// Returns true if the reactor believes that the source is currently
    /// writable.
    pub fn is_writable(&self) -> bool {
        let is_writable = self.inner.readiness.get().is_writable();

        if !is_writable {
            self.interest(Ready::writable());
        }

        is_writable
    }

    /// Notify the reactor that the source is currently not writable.
    pub fn unset_writable(&self) {
        self.interest(Ready::writable());
        let readiness = self.inner.readiness.get();
        self.inner.readiness.set(readiness - Ready::writable());
    }

    /// Notify the reactor that the currently running FSM has expressed
    /// interest in the source.
    pub fn advance(&self) {
        EventLoop::current_task_did_advance()
    }

    /// Notify the reactor that the currently running FSM has expressed
    /// interest in the source.
    fn interest(&self, interest: Ready) {
        let task = match EventLoop::current_task(self.inner.event_loop) {
            Some(t) => t,
            None => return,
        };

        debug!("registering interest in source; interest={:?}; current={:?}; token={:?}; task={:?}",
               interest, self.inner.readiness.get(), self.inner.token, task);

        self.inner.task.set(Some(task.token()));

        let interest_at = task.tick_num();
        let last_interest_at = self.inner.interest_at.get();

        if last_interest_at < interest_at {
            self.inner.interest.set(interest);
        } else {
            self.inner.interest.set(interest | self.inner.interest.get());
        }
    }
}

impl Drop for Source {
    fn drop(&mut self) {
        EventLoop::drop_source(self.inner.token);
    }
}

/*
 *
 * ===== Reactor private functions =====
 *
 */

/// Return a new Source with the given token and reactor handle
pub fn new(token: Token, event_loop: EventLoopId) -> Weak {
    let inner = Rc::new(Inner {
        readiness: Cell::new(Ready::none()),
        token: token,
        task: Cell::new(None),
        interest: Cell::new(Ready::none()),
        interest_at: Cell::new(0),
        event_loop: event_loop,
    });

    Weak { inner: inner }
}

/// Returns the token associated with the source
pub fn token(source: &Source) -> Token {
    source.inner.token
}

impl Weak {
    pub fn task(&self) -> Option<Token> {
        self.inner.task.get()
    }

    /// Returns the source's readiness without expressing interest
    pub fn peek_readiness(&self) -> Ready {
        self.inner.readiness.get()
    }

    /// Sets the source's readiness. This should only be called by the reactor
    /// and does not express any interest.
    pub fn set_readiness(&self, val: Ready) {
        self.inner.readiness.set(val);
    }

    pub fn strong(&self) -> Source {
        Source { inner: self.inner.clone() }
    }
}
