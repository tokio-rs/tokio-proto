use take::Take;
use std::io;

/// A non-blocking unit of work scheduled to run on a `Reactor`.
///
/// For more details, read the module level documentation.
pub trait Task {

    /// Advance the state of the task, aka do work.
    ///
    /// This function is called by the `Reactor` when at least one source
    /// blocking the task from making forward progress is ready.
    ///
    /// For example, if the task is waiting on a `TcpStream` to read data, when
    /// the reactor sees that the `TcpStream` has data ready to read, it will
    /// call `tick` on the task.
    fn tick(&mut self) -> io::Result<Tick>;

    /// Returns true if the `Task` will return `Tick::Final` immediately.
    ///
    /// This is used by `Reactor` to avoid allocating resources to track the
    /// `Task` after the first tick invocation.
    fn oneshot(&self) -> bool {
        false
    }
}

/// Informs the `Reactor` how to process the current task
pub enum Tick {
    /// The task would have blocked and has more work to do
    WouldBlock,
    /// The task has completed all work
    Final,
}

impl<F, T> Task for F
    where F: FnMut() -> T,
          T: IntoTick,
{
    fn tick(&mut self) -> io::Result<Tick> {
        self().into_tick()
    }

    fn oneshot(&self) -> bool {
        T::oneshot()
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

/// A conversion into `Tick`
pub trait IntoTick {
    /// Convert the value into an `io::Result<Tick>`
    fn into_tick(self) -> io::Result<Tick>;

    /// Returns `true` if the return type implies a `oneshot` dispatch
    fn oneshot() -> bool;
}

impl IntoTick for () {
    fn into_tick(self) -> io::Result<Tick> {
        Ok(Tick::Final)
    }

    fn oneshot() -> bool {
        true
    }
}

impl IntoTick for io::Result<()> {
    fn into_tick(self) -> io::Result<Tick> {
        Ok(Tick::Final)
    }

    fn oneshot() -> bool {
        true
    }
}

impl IntoTick for io::Result<Tick> {
    fn into_tick(self) -> io::Result<Tick> {
        self
    }

    fn oneshot() -> bool {
        false
    }
}
