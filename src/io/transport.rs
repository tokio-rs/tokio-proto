// TODO: Should Transport::write() internally call flush() or should the API
// document that users of Transport should call flush() after writing?

use io::Readiness;
use std::io;


/// A typed stream that may be read from and written to without blocking.
///
/// Most transports to protocol level serialization and deserialization and are
/// backed by a `TcpStream`.
///
/// For more details, read the module level documentation.
pub trait Transport: Readiness {
    /// Messages written to the transport
    type In;

    /// Messages read from the transport
    type Out;

    /// Read a message frame from the `Transport`
    fn read(&mut self) -> io::Result<Option<Self::Out>>;

    /// Write a message frame to the `Transport`
    fn write(&mut self, req: Self::In) -> io::Result<Option<()>>;

    /// Flush pending writes to the socket
    ///
    /// Since the backing source is non-blocking, there is no guarantee that a
    /// call to `Transport::write` is able to write the full message to the
    /// backing source immediately. In this case, the transport will need to
    /// buffer the remaining data to write. Calls to `Transport:flush` attempt
    /// to write any remaining data in the write buffer to the underlying
    /// source.
    fn flush(&mut self) -> io::Result<Option<()>>;
}
