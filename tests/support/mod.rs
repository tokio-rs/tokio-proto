#![allow(dead_code)]

extern crate futures;
extern crate tokio_service;

use std::marker;
use std::io;

use self::tokio_service::Service;
use self::futures::{IntoFuture, Future};

pub mod mock;
pub mod multiplex;

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

pub struct FnService<T, U> {
    inner: Box<Fn(T) -> Box<Future<Item=U, Error=io::Error> + Send> + Sync + Send>,
}

impl<T, U> FnService<T, U> {
    pub fn new<F, R>(f: F) -> FnService<T, U>
        where F: Fn(T) -> R + Send + Sync + 'static,
              R: IntoFuture<Item=U, Error=io::Error>,
              R::Future: Send + 'static,

    {
        FnService {
            inner: Box::new(move |t| Box::new(f(t).into_future())),
        }
    }
}

impl<T, U> Service for FnService<T, U> {
    type Request = T;
    type Response = U;
    type Future = Box<Future<Item=U, Error=io::Error> + Send>;
    type Error = io::Error;

    fn call(&self, t: T) -> Self::Future {
        (self.inner)(t)
    }
}
