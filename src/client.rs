//! A generic Tokio TCP client implementation.

use tcp::TcpStream;
use reactor::{self, ReactorHandle, Task, NewTask};
use std::net::SocketAddr;

/// Connect to the given `addr` then handle the `TcpStream` on the task created
/// by `new_task`.
///
/// ```rust,no_run
/// use tokio::client;
/// use tokio::io::TryWrite;
/// use tokio::reactor::*;
/// use tokio::tcp::TcpStream;
/// use std::io;
///
/// struct Connection {
///     stream: TcpStream,
/// };
///
/// impl Connection {
///     fn new(stream: TcpStream) -> Connection {
///         Connection { stream: stream }
///     }
/// }
///
/// impl Task for Connection {
///     fn tick(&mut self) -> io::Result<Tick> {
///         // Write a single byte to the socket then shutdown
///         //
///         if let Some(1) = try!(self.stream.try_write(b"a")) {
///             // The byte was written, shutdown
///             return Ok(Tick::Final);
///         }
///
///         // Write did not succeed, try again
///         Ok(Tick::WouldBlock)
///     }
/// }
///
/// let reactor = Reactor::default().unwrap();
///
/// // Start the client
/// client::connect(&reactor.handle(),
///                "127.0.0.1:3245".parse().unwrap(),
///                |stream| Ok(Connection::new(stream)));
///
/// // Run the reactor
/// reactor.run().unwrap();
///
/// ```
pub fn connect<T>(reactor: &ReactorHandle, addr: SocketAddr, new_task: T)
        where T: NewTask
{
    reactor.oneshot(move || {
        // Create a new Tokio TcpStream from the Mio socket
        let socket = match TcpStream::connect(&addr) {
            Ok(s) => s,
            Err(_) => unimplemented!(),
        };

        let task = match new_task.new_task(socket) {
            Ok(d) => d,
            Err(_) => unimplemented!(),
        };

        reactor::schedule(task);
    });
}
