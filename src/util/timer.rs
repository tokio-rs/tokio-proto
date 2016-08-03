//! A Tokio aware timer backed by Mio's hashed wheel timer.

use io::Ready;
use reactor::{self, Source};
use mio::timer as mio;
use std::io;
use std::time::Duration;

/// A Tokio aware timer backed by Mio's hashed wheel timer.
pub struct Timer<T> {
    mio: mio::Timer<T>,
    source: Source,
}

pub use mio::{Timeout, TimerError};

/// A specialized `Result` type for timer operations.
pub type Result<T> = ::std::result::Result<T, TimerError>;

impl<T> Timer<T> {
    /// Create and return a new Tokio aware `Receiver` backed by the given Mio
    /// receiver
    pub fn watch(mio: mio::Timer<T>) -> io::Result<Timer<T>> {
        let source = try!(reactor::register_source(&mio, Ready::readable()));

        Ok(Timer {
            mio: mio,
            source: source,
        })
    }

    /// Set a timeout for the given delay, after which a call to `poll` will
    /// return the provided `val`.
    ///
    /// A `Timeout` token is returned which can be used to cancel the timeout
    /// before it is triggered.
    pub fn set_timeout(&mut self, delay_from_now: Duration, val: T) -> Result<Timeout> {
        self.mio.set_timeout(delay_from_now, val)
    }

    /// Cancel the timeout associated with the given `Timeout` token.
    ///
    /// If the timeout has already been triggered, `None` will be returned,
    /// otherwise, the `val` originally provided to `set_timeout` is returned.
    pub fn cancel_timeout(&mut self, timeout: &Timeout) -> Option<T> {
        self.mio.cancel_timeout(timeout)
    }

    /// Query the timer for the next expired timeout.
    ///
    /// If a timeout has been triggered, the `val` originally provided to
    /// `set_timeout` is returned, otherwise, `None` is returned.
    pub fn poll(&mut self) -> Option<T> {
        if !self.source.is_readable() {
            return None;
        }

        match self.mio.poll() {
            Some(v) => {
                self.source.advance();
                Some(v)
            }
            None => {
                self.source.unset_readable();
                None
            }
        }
    }
}
