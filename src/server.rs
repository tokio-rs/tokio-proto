//! A generic Tokio TCP server implementation.

use std::io;
use std::net::SocketAddr;

use futures::stream::Stream;
use futures::{self, Future};
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
    type Item: Future<Item=(), Error=io::Error> + 'static;

    /// Create and return a new `Task` value
    fn new_task(&self, stream: TcpStream) -> io::Result<Self::Item>;
}

/// Spawn a new `Task` that binds to the given `addr` then accepts all incoming
/// connections; dispatching them to tasks created by `new_task`.
///
/// ```rust,no_run
/// extern crate futures;
/// extern crate tokio;
/// #[macro_use]
/// extern crate tokio_core;
///
/// use std::io::{self, Read};
///
/// use futures::{Future, Poll};
/// use tokio::server;
/// use tokio_core::{Loop, TcpStream};
///
/// struct Connection {
///     stream: TcpStream,
///     buf: Box<[u8]>,
/// }
///
/// impl Connection {
///     fn new(stream: TcpStream) -> Connection {
///         let buf = vec![0; 1024];
///
///         Connection {
///             stream: stream,
///             buf: buf.into_boxed_slice(),
///         }
///     }
/// }
///
/// impl Future for Connection {
///     type Item = ();
///     type Error = io::Error;
///
///     fn poll(&mut self) -> Poll<(), io::Error> {
///         loop {
///             let n = try_nb!(self.stream.read(&mut self.buf));
///             println!("read {} bytes", n);
///
///             if n == 0 {
///                 // Socket closed, shutdown
///                 return Poll::Ok(())
///             }
///         }
///
///         Poll::NotReady
///     }
/// }
///
/// fn main() {
///     let mut lp = Loop::new().unwrap();
///
///     // Launch the server
///     server::listen(lp.handle(),
///                    "0.0.0.0:3245".parse().unwrap(),
///                    |stream| Ok(Connection::new(stream)));
///
///     // Run the reactor
///     lp.run(futures::empty::<(), ()>()).unwrap();
/// }
/// ```
pub fn listen<T>(handle: LoopHandle,
                 addr: SocketAddr,
                 new_task: T) -> IoFuture<ServerHandle>
    where T: NewTask
{
    let new_task = handle.add_loop_data(|p| {
        futures::finished::<_, io::Error>((new_task, p.clone()))
    });
    let listener = handle.clone().tcp_listen(&addr);
    listener.join(new_task).and_then(move |(socket, new_task)| {
        let addr = try!(socket.local_addr());

        new_task.and_then(|(new_task, p)| {
            let p2 = p.clone();
            let res = socket.incoming().for_each(move |(socket, _)| {
                let task = new_task.new_task(socket);
                p2.add_loop_data(futures::done(task).flatten()).forget();
                Ok(())
            });
            p.add_loop_data(res)
        }).forget();

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
          U: Future<Item=(), Error=io::Error> + 'static,
{
    type Item = U;

    fn new_task(&self, stream: TcpStream) -> io::Result<Self::Item> {
        self(stream)
    }
}

impl<T, U> NewTask for Take<T>
    where T: FnOnce(TcpStream) -> io::Result<U> + Send + 'static,
          U: Future<Item=(), Error=io::Error> + 'static,
{
    type Item = U;

    fn new_task(&self, stream: TcpStream) -> io::Result<U> {
        self.take()(stream)
    }
}
