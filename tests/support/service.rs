extern crate futures;
extern crate tokio_service;

use std::io;

use self::futures::{Future, IntoFuture};
use self::tokio_service::Service;

pub struct SimpleService<T, U> {
    inner: Box<Fn(T) -> Box<Future<Item=U, Error=io::Error> + Send> + Send>,
}

/// Returns a `Service` backed by the given closure.
pub fn simple_service<F, R, T, U>(f: F) -> SimpleService<T, U>
    where F: Fn(T) -> R + Send + 'static,
          R: IntoFuture<Item=U, Error=io::Error>,
          R::Future: Send + 'static,
{
    SimpleService {
        inner: Box::new(move |t| Box::new(f(t).into_future())),
    }
}

impl<T, U> Service for SimpleService<T, U> {
    type Request = T;
    type Response = U;
    type Future = Box<Future<Item=U, Error=io::Error> + Send>;
    type Error = io::Error;

    fn call(&self, t: T) -> Self::Future {
        (self.inner)(t)
    }
}
