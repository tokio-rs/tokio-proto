mod futures;

pub mod mock;

pub use self::futures::await;

use std::time::Duration;

/// Block the thread for the given number of milliseconds
pub fn sleep_ms(ms: u64) {
    use std::thread;
    thread::sleep(millis(ms));
}

/// Return a `Duration` representing the given number of milliseconds
pub fn millis(ms: u64) -> Duration {
    Duration::from_millis(ms)
}
