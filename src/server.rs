//! A generic Tokio TCP server implementation.

use tcp::{TcpListener, TcpStream};
use reactor::{self, ReactorHandle, Task, Tick};
use mio::tcp as mio;
use take::Take;
use std::io;
use std::net::SocketAddr;

/// A handle to a running server.
pub struct ServerHandle {
    local_addr: SocketAddr,
}

struct Listener<T> {
    socket: TcpListener,
    new_task: T,
}



/// Create a new `Task` to handle a server socket.
pub trait NewTask: Send + 'static {
    /// The `Task` value created by this factory
    type Item: Task;

    /// Create and return a new `Task` value
    fn new_task(&self, stream: TcpStream) -> io::Result<Self::Item>;
}

/// Spawn a new `Task` that binds to the given `addr` then accepts all incoming
/// connections; dispatching them to tasks created by `new_task`.
///
/// ```rust,no_run
/// use tokio::server;
/// use tokio::io::TryRead;
/// use tokio::tcp::TcpStream;
/// use tokio::reactor::*;
/// use std::io;
///
/// struct Connection {
///     stream: TcpStream,
///     buf: Box<[u8]>,
/// };
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
/// impl Task for Connection {
///     fn tick(&mut self) -> io::Result<Tick> {
///         while let Some(n) = try!(self.stream.try_read(&mut self.buf)) {
///             println!("read {} bytes", n);
///
///             if n == 0 {
///                 // Socket closed, shutdown
///                 return Ok(Tick::Final);
///             }
///         }
///
///         Ok(Tick::WouldBlock)
///     }
/// }
///
/// let reactor = Reactor::default().unwrap();
///
/// // Launch the server
/// server::listen(&reactor.handle(),
///                "0.0.0.0:3245".parse().unwrap(),
///                |stream| Ok(Connection::new(stream)));
///
/// // Run the reactor
/// reactor.run().unwrap();
///
/// ```
pub fn listen<T>(reactor: &ReactorHandle, addr: SocketAddr, new_task: T) -> io::Result<ServerHandle>
        where T: NewTask
{

    let socket = try!(mio::TcpListener::bind(&addr));
    let addr = try!(socket.local_addr());

    reactor.oneshot(move || {
        // Create a new Tokio TcpListener from the Mio socket
        let socket = match TcpListener::watch(socket) {
            Ok(s) => s,
            Err(_) => unimplemented!(),
        };

        // Initialize the new listener
        let listener = Listener::new(socket, new_task);

        // Register the listener with the Reactor
        try!(reactor::schedule(listener));
        Ok(())
    });

    Ok(ServerHandle {
        local_addr: addr,
    })
}

impl ServerHandle {
    /// Returns the local socket address of the `TcpListener` for this server.
    pub fn local_addr(&self) -> &SocketAddr {
        &self.local_addr
    }
}

impl<T> Listener<T> {
    fn new(socket: TcpListener, new_task: T) -> Listener<T> {
        Listener {
            socket: socket,
            new_task: new_task,
        }
    }
}

impl<T: NewTask> Task for Listener<T> {
    fn tick(&mut self) -> io::Result<Tick> {
        debug!("listener task ticked");

        // As long as there are sockets to accept, accept and process them
        while let Some(socket) = try!(self.socket.accept()) {
            trace!("accepted new TCP connection");
            let socket = try!(TcpStream::watch(socket));
            let task = try!(self.new_task.new_task(socket));

            try!(reactor::schedule(task));
        }

        Ok(Tick::WouldBlock)
    }
}

impl<T, U> NewTask for T
    where T: Fn(TcpStream) -> io::Result<U> + Send + 'static,
          U: Task,
{
    type Item = U;

    fn new_task(&self, stream: TcpStream) -> io::Result<Self::Item> {
        self(stream)
    }
}

impl<T, U> NewTask for Take<T>
    where T: FnOnce(TcpStream) -> io::Result<U> + Send + 'static,
          U: Task,
{
    type Item = U;

    fn new_task(&self, stream: TcpStream) -> io::Result<U> {
        self.take()(stream)
    }
}
