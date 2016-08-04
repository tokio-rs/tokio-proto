//! A Tokio aware channel `Receiver` wrapping a `mio::Receiver`.

use io::{Ready, Readiness};
use reactor::{self, Source};
use mio::channel as mio;
use std::io;
use std::sync::mpsc::{RecvError, TryRecvError};

/// A Tokio aware channel `Receiver` wrapping a `mio::Receiver`.
pub struct Receiver<T> {
    mio: mio::Receiver<T>,
    source: Source,
}

impl<T> Receiver<T> {
    /// Create and return a new Tokio aware `Receiver` backed by the given Mio
    /// receiver
    pub fn watch(mio: mio::Receiver<T>) -> io::Result<Receiver<T>> {
        let source = try!(reactor::register_source(&mio, Ready::readable()));

        Ok(Receiver {
            mio: mio,
            source: source,
        })
    }

    /// Attempts to return a pending value on this receiver without blocking.
    ///
    /// This method will never block the caller in order to wait for data to
    /// become available. Instead, it will return `Ok(None)` and register
    /// interest with the `Reactor`.
    pub fn recv(&self) -> Result<Option<T>, RecvError> {
        if !self.source.is_readable() {
            return Ok(None);
        }

        match self.mio.try_recv() {
            Ok(val) => {
                self.source.advance();
                Ok(Some(val))
            }
            Err(TryRecvError::Empty) => {
                self.source.unset_readable();
                Ok(None)
            }
            Err(TryRecvError::Disconnected) => {
                self.source.advance();
                Err(RecvError)
            }
        }
    }
}

impl<T> Readiness for Receiver<T> {
    fn is_readable(&self) -> bool {
        self.source.is_readable()
    }

    fn is_writable(&self) -> bool {
        self.source.is_writable()
    }
}
