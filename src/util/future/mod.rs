//! Utilities useful for working with Tokio and futures.
//!
//! # Waiting
//!
//! Anything that would require waiting for a value on a `Task` needs to be
//! Tokio aware, including waiting for futures to complete. `Await` (to be
//! written) and `AwaitQueue` provide facilities for notifying the Reactor when
//! a future completes and may be polled.

mod queue;
mod val;

pub use self::queue::AwaitQueue;
pub use self::val::{pair, Complete, Cancellation, Val};
