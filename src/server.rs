//! A generic Tokio TCP server implementation.

use std::io;
use std::net::SocketAddr;

use futures::stream::Stream;
use futures::Future;
use take::Take;
use tokio_core::io::IoFuture;
use tokio_core::{TcpStream, LoopHandle};

/// A handle to a running server.
pub struct ServerHandle {
    local_addr: SocketAddr,
}

/// Create a new `Task` to handle a server socket.
pub trait NewTask: Send + 'static {
    /// The `Task` value created by this factory
    type Item: Future<Item=(), Error=io::Error>;

    /// Create and return a new `Task` value
    fn new_task(&self, stream: TcpStream) -> io::Result<Self::Item>;
}

/// Spawn a new `Task` that binds to the given `addr` then accepts all incoming
/// connections; dispatching them to tasks created by `new_task`.
///
/// ```rust,no_run
/// extern crate futures;
/// extern crate tokio_proto;
/// extern crate tokio_core;
///
/// use futures::Future;
/// use futures::stream::Stream;
/// use tokio_core::Loop;
///
/// pub fn main() {
///     // Create a new loop
///     let mut lp = Loop::new().unwrap();
///
///     // Bind to port 4000
///     let addr = "0.0.0.0:4000".parse().unwrap();
///
///     // Create the new TCP listener
///     let listener = lp.handle().tcp_listen(&addr);
///
///     let srv = listener.and_then(|l| {
///         // Accept each incoming connection
///         l.incoming().for_each(|socket| {
///             // Do something with the socket
///             println!("{:#?}", socket);
///             Ok(())
///         })
///     });
///
///     println!("listening on {:?}", addr);
///
///     lp.run(srv).unwrap();
/// }
/// ```
pub fn listen<T>(handle: LoopHandle,
                 addr: SocketAddr,
                 new_task: T) -> IoFuture<ServerHandle>
    where T: NewTask
{
    let listener = handle.clone().tcp_listen(&addr);
    listener.and_then(move |socket| {
        let addr = try!(socket.local_addr());

        handle.spawn(|pin| {
            let pin = pin.clone();
            socket.incoming().for_each(move |(socket, _)| {
                let task = try!(new_task.new_task(socket));
                // TODO: where to punt this error to?
                pin.spawn(task.map_err(|e| {
                    error!("task error: {}", e);
                }));
                Ok(())
            }).map_err(|e| {
                // TODO: where to punt this error to?
                error!("server error: {}", e);
            })
        });

        Ok(ServerHandle { local_addr: addr })
    }).boxed()
}

impl ServerHandle {
    /// Returns the local socket address of the `TcpListener` for this server.
    pub fn local_addr(&self) -> &SocketAddr {
        &self.local_addr
    }
}

impl<T, U> NewTask for T
    where T: Fn(TcpStream) -> io::Result<U> + Send + 'static,
          U: Future<Item=(), Error=io::Error>,
{
    type Item = U;

    fn new_task(&self, stream: TcpStream) -> io::Result<Self::Item> {
        self(stream)
    }
}

impl<T, U> NewTask for Take<T>
    where T: FnOnce(TcpStream) -> io::Result<U> + Send + 'static,
          U: Future<Item=(), Error=io::Error>,
{
    type Item = U;

    fn new_task(&self, stream: TcpStream) -> io::Result<U> {
        self.take()(stream)
    }
}
