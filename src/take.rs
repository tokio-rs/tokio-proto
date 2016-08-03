//! Cell allowing the inner value to be consumed without a mutable reference.
//!
//! In order to maintain safety, it is not possible to get a reference to the
//! inner value.

use reactor::{Task, NewTask, Tick, IntoTick};
use tcp::TcpStream;
use std::io;
use std::cell::UnsafeCell;

/// Cell allowing the inner value to be consumed without a mutable reference.
pub struct Take<F> {
    val: UnsafeCell<Option<F>>,
}

impl<T> Take<T> {
    /// Create and return a new `Take` value containing the given inner value.
    pub fn new(val: T) -> Take<T> {
        Take { val: UnsafeCell::new(Some(val)) }
    }

    /// Consume and return the inner value.
    ///
    /// # Panics
    ///
    /// If the inner value has already been consumed, the call will panic.
    fn take(&self) -> T {
        unsafe { (*self.val.get()).take() }.expect("value already consumed")
    }
}

impl<F, T> Task for Take<F>
    where F: FnOnce() -> T,
          T: IntoTick,
{
    fn tick(&mut self) -> io::Result<Tick> {
        self.take()().into_tick()
    }

    fn oneshot(&self) -> bool {
        true
    }
}

impl<T, U> NewTask for Take<T>
    where T: FnOnce(TcpStream) -> io::Result<U> + Send + 'static,
          U: Task,
{
    type Item = U;

    fn new_task(&self, stream: TcpStream) -> io::Result<U> {
        self.take()(stream)
    }
}
