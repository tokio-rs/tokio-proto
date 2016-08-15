//! Tokio aware UDP primitives

use io::{Readiness, Ready};
use reactor::{self, Source};
use mio::would_block;
use mio::udp as mio;
use std::io;
use std::net::SocketAddr;

/// A UDP socket.
pub struct UdpSocket {
    mio: mio::UdpSocket,
    source: Source,
}

impl UdpSocket {

    /// Creates a UDP socket from the given address.
    pub fn bind(addr: &SocketAddr) -> io::Result<UdpSocket> {
        let mio = try!(mio::UdpSocket::bind(addr));
        UdpSocket::watch(mio)
    }

    /// Create and return a new `UdpSocket` backed by the given Mio
    /// UdpSocket.
    ///
    /// This turns an existing socket into a Tokio aware socket.
    pub fn watch(mio: mio::UdpSocket) -> io::Result<UdpSocket> {
        let source = try!(reactor::register_source(&mio, Ready::all()));

        Ok(UdpSocket {
            mio: mio,
            source: source,
        })
    }

    /// Returns the local address of this socket.
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        self.mio.local_addr()
    }

    /// Sends data on the socket to the given address. On success, returns the
    /// number of bytes written.
    pub fn send_to(&self, buf: &[u8], target: &SocketAddr) -> io::Result<usize> {
        if !self.source.is_writable() {
            return Err(would_block());
        }

        match self.mio.send_to(buf, target) {
            Ok(Some(n)) => {
                self.source.advance();
                Ok(n)
            }
            Ok(None) => {
                self.source.unset_writable();
                Err(would_block())
            }
            Err(e) => {
                self.source.advance();
                Err(e)
            }
        }
    }

    /// Receives data from the socket. On success, returns the number of bytes
    /// read and the address from whence the data came.
    pub fn recv_from(&self, buf: &mut [u8]) -> io::Result<(usize, SocketAddr)> {
        if !self.source.is_readable() {
            return Err(would_block());
        }

        match self.mio.recv_from(buf) {
            Ok(Some(n)) => {
                self.source.advance();
                Ok(n)
            }
            Ok(None) => {
                self.source.unset_readable();
                Err(would_block())
            }
            Err(e) => {
                self.source.advance();
                Err(e)
            }
        }
    }
}

impl Readiness for UdpSocket {
    fn is_readable(&self) -> bool {
        self.source.is_readable()
    }

    fn is_writable(&self) -> bool {
        self.source.is_writable()
    }
}
