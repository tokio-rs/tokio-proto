//! The non-blocking event driven core of Tokio
//!
//! `Reactor` is the backbone of Tokio's non-blocking evented system. Tasks are
//! scheduled on the reactor, which then detect which sources each task needs
//! in order to make progress, waits for readiness, then executes the task.
//!
//! # Tasks
//!
//! A task is a small, non-blocking, unit of work. A common example would be
//! having a task manage a single TCP socket.
//!
//! A task implements a single function: `Task::tick() -> Tick`. All work that
//! the task needs to get done happens here. It is important that a task never
//! block. Blocking a task means that the entire reactor is blocked, including
//! all other tasks managed by the reactor.
//!
//! A hello world task would look something like:
//!
//! ```rust
//! use tokio::reactor::*;
//! use std::io;
//!
//! struct HelloWorld;
//!
//! impl Task for HelloWorld {
//!     fn tick(&mut self) -> io::Result<Tick> {
//!         println!("Hello world!");
//!         Ok(Tick::Final)
//!     }
//! }
//! ```
//!
//! Tasks are scheduled on the reactor by calling
//! `ReactorHandle::schedule(&self, task)`.
//!
//! # Waiting
//!
//! Since a task may not block the thread, a different strategy is employed. If
//! the task is unable to make further progress due to having to being blocked
//! on an external source, like a TCP socket, the task returns
//! `Ok(Tick::WouldBlock)`.
//!
//! When a task returns `Tick::WouldBlock`, the reactor knows that the task has
//! more work to do. It then waits until the sources that are currently
//! blocking the task from making progress are ready. When the sources become
//! ready, the reactor schedules the task for execution again, at which
//! time the Task will be able to make further progress.
//!
//! When the task has completed its work, it returns `Ok(Tick::Final)`, at
//! which time the reactor will drop the task and it will no longer be called
//! again.
//!
//! # Sources
//!
//! A source is any value that a task can depend on for input. Sources included
//! with Tokio currently include:
//!
//! * `TcpListener` and `TcpStream` for working with the TCP protocol.
//! * `AwaitQueue` for waiting for the completion of a future
//! * `Receiver` for waiting for messages on a channel.
//! * `Timer` for getting notified after a set time interval.
//!
//! However, it is possible to implement custom sources.
//!
//! A Source must be able to notify the reactor that a task depends on the
//! source for making forward progress. This can be done with the `Source`
//! type.
//!
//! # Scheduling
//!
//! The reactor schedules a task for execution as long as it is able to make
//! forward progress. Being able to make forward progress is defined as:
//!
//! Either:
//!
//!   * No source was used during a call to `tick`
//!   * Sources were used, and **at least** one source was ready to complete
//!   the operation.
//!
//! If a task accessed at least one source, and none of the sources were ready
//! to complete the operation, then the reactor automatically registers
//! interest for those sources. As soon as any of those sources transition to a
//! ready state, the reactor will schedule the task for execution again.
//!
//! # Notes
//!
//! A few notes on implementing tasks:
//!
//! * The reactor may call the task spurriously. A call to `Task::tick` does
//! not guarantee that any of the sources will be ready to operate on.
//! * A source will not be considered as blocking a task until it returns
//! `Ok(None)` would-block. Given this, it is important to consume the source
//! as long as it is ready.

mod reactor;
mod source;
mod task;

pub use self::reactor::{
    Config,
    Reactor,
    ReactorHandle,
    Error,
    Result,
    schedule,
    oneshot,
    register_source,
    shutdown,
};
pub use self::source::Source;
pub use self::task::{
    Task,
    Tick,
    IntoTick,
};
