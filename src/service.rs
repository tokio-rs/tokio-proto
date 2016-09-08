use std::io;

use tokio_service::Service;

/// Creates new `Service` values.
pub trait NewService {

    /// Requests handled by the service
    type Request;

    /// Responses given by the service
    type Response;

    /// Errors produced by the service
    type Error;

    /// The `Service` value created by this factory
    type Item: Service<Request = Self::Request, Response = Self::Response, Error = Self::Error>;

    /// Create and return a new service value.
    fn new_service(&self) -> io::Result<Self::Item>;
}

impl<T> NewService for T
    where T: Service + Clone,
{
    type Item = T;
    type Request = T::Request;
    type Response = T::Response;
    type Error = T::Error;

    fn new_service(&self) -> io::Result<T> {
        Ok(self.clone())
    }
}
