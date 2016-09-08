use std::io;

use tokio_service::Service;

/// Creates new `Service` values.
pub trait NewService {

    /// Requests handled by the service
    type Req;

    /// Responses given by the service
    type Resp;

    /// Errors produced by the service
    type Error;

    /// The `Service` value created by this factory
    type Item: Service<Req = Self::Req, Resp = Self::Resp, Error = Self::Error>;

    /// Create and return a new service value.
    fn new_service(&self) -> io::Result<Self::Item>;
}

impl<T> NewService for T
    where T: Service + Clone,
{
    type Item = T;
    type Req = T::Req;
    type Resp = T::Resp;
    type Error = T::Error;

    fn new_service(&self) -> io::Result<T> {
        Ok(self.clone())
    }
}
