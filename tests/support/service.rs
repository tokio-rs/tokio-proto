extern crate tokio_service;

use self::tokio_service::Service;
use futures::{Future, IntoFuture};
use std::marker::PhantomData;
use std::sync::Arc;

/// Returns a `Service` backed by the given closure.
pub fn simple_service<F, R>(f: F) -> SimpleService<F, R> {
    SimpleService::new(f)
}

pub struct SimpleService<F, R> {
    f: Arc<F>,
    _ty: PhantomData<fn() -> R>, // don't impose Sync on R
}


impl<F, R> SimpleService<F, R> {
    /// Create and return a new `SimpleService` backed by the given function.
    pub fn new(f: F) -> SimpleService<F, R> {
        SimpleService {
            f: Arc::new(f),
            _ty: PhantomData,
        }
    }
}

impl<F, R, S> Service for SimpleService<F, R>
    where F: Fn(R) -> S + Sync + Send + 'static,
          R: Send + 'static,
          S: IntoFuture + Send + 'static,
          S::Future: Send + 'static,
          <S::Future as Future>::Item: Send + 'static,
          <S::Future as Future>::Error: Send + 'static,
{
    type Request = R;
    type Response = S::Item;
    type Error = S::Error;
    type Future = S::Future;

    fn call(&self, req: R) -> Self::Future {
        (self.f)(req).into_future()
    }
}
