//! Tokio aware TCP primitives

use io::{Readiness, Ready};
use reactor::{self, Source};
use mio::would_block;
use mio::tcp as mio;
use std::io::{self, Read, Write};
use std::net::SocketAddr;

/// A TCP server socket.
pub struct TcpListener {
    mio: mio::TcpListener,
    source: Source,
}

impl TcpListener {

    /// Creates a new `TcpListener` which will be bound to the specified address.
    ///
    /// The returned listener is ready for accepting connections.
    ///
    /// Binding with a port number of 0 will request that the OS assigns a port
    /// to this listener. The port allocated can be queried via the local_addr
    /// method.
    pub fn bind(addr: &SocketAddr) -> io::Result<TcpListener> {
        let mio = try!(mio::TcpListener::bind(addr));
        TcpListener::watch(mio)
    }

    /// Create and return a new `TcpListener` backed by the given Mio
    /// TcpListener.
    ///
    /// This turns an existing listener into a Tokio aware listener.
    pub fn watch(mio: mio::TcpListener) -> io::Result<TcpListener> {
        let source = try!(reactor::register_source(&mio, Ready::readable()));

        Ok(TcpListener {
            mio: mio,
            source: source,
        })
    }

    /// Returns the local socket address of this listener.
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.mio.local_addr()
    }

    /// Accept a new incoming connection from this listener.
    ///
    /// This is a non-blocking function. If a new TCP connection is ready to be
    /// established, `Ok(Some(new_stream))` is returned. If there is no pending
    /// TCP connection, `Ok(None)` is returned. In the event of an error,
    /// `Err(error)` is returned.
    pub fn accept(&self) -> io::Result<Option<mio::TcpStream>> {
        if !self.source.is_readable() {
            return Ok(None);
        }

        match self.mio.accept() {
            Ok(Some((socket, _))) => {
                self.source.advance();
                Ok(Some(socket))
            }
            Ok(None) => {
                self.source.unset_readable();
                Ok(None)
            }
            Err(e) => {
                self.source.advance();
                Err(e)
            }
        }
    }
}

impl Readiness for TcpListener {
    fn is_readable(&self) -> bool {
        self.source.is_readable()
    }

    fn is_writable(&self) -> bool {
        false
    }
}

/// A TCP stream between a local socket and a remote socket.
pub struct TcpStream {
    mio: mio::TcpStream,
    source: Source,
}

impl TcpStream {
    /// Opens a TCP connection to a remote host.
    ///
    /// `addr` is an address of the remote host. This is a non-blocking
    /// function. The TCP connection may or may not be established when the
    /// `TcpStream` is returned.
    ///
    /// If the connection is not yet established, calls to `read` and `write`
    /// will return `Ok(None)`.
    pub fn connect(addr: &SocketAddr) -> io::Result<TcpStream> {
        let mio = try!(mio::TcpStream::connect(addr));
        TcpStream::watch(mio)
    }

    /// Create and return a new `TcpStream` backed by the given Mio
    /// TcpStream.
    ///
    /// This turns an existing stream into a Tokio aware stream.
    pub fn watch(mio: mio::TcpStream) -> io::Result<TcpStream> {
        let source = try!(reactor::register_source(&mio, Ready::all()));

        Ok(TcpStream {
            mio: mio,
            source: source,
        })
    }

    /// Pull some bytes from this stream into the specified buffer, returning
    /// how many bytes were read.
    ///
    /// For more details, see the `std::io::Read::read` documentation.
    pub fn read(&self, buf: &mut [u8]) -> io::Result<usize> {
        if !self.source.is_readable() {
            return Err(would_block());
        }

        match (&self.mio).read(buf) {
            Ok(n) => {
                self.source.advance();
                Ok(n)
            }
            Err(e) => {
                if e.kind() == io::ErrorKind::WouldBlock {
                    self.source.unset_readable();
                } else {
                    self.source.advance();
                }

                Err(e)
            }
        }
    }

    /// Write a buffer to this stream, returning how many bytes were written.
    ///
    /// For more details, see the `std::io::Write::write` documentation.
    pub fn write(&self, buf: &[u8]) -> io::Result<usize> {
        if !self.source.is_writable() {
            return Err(would_block());
        }

        match (&self.mio).write(buf) {
            Ok(n) => {
                self.source.advance();
                Ok(n)
            }
            Err(e) => {
                if e.kind() == io::ErrorKind::WouldBlock {
                    self.source.unset_writable();
                } else {
                    self.source.advance();
                }

                Err(e)
            }
        }
    }
}

impl Readiness for TcpStream {
    fn is_readable(&self) -> bool {
        self.source.is_readable()
    }

    fn is_writable(&self) -> bool {
        self.source.is_writable()
    }
}

impl io::Read for TcpStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        TcpStream::read(&*self, buf)
    }
}

impl io::Write for TcpStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        TcpStream::write(&*self, buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        // Does nothing
        Ok(())
    }
}

impl<'a> io::Read for &'a TcpStream {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        TcpStream::read(self, buf)
    }
}

impl<'a> io::Write for &'a TcpStream {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        TcpStream::write(self, buf)
    }

    fn flush(&mut self) -> io::Result<()> {
        // Does nothing
        Ok(())
    }
}
