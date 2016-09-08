use std::{fmt, ops};

use tokio_core::net::{TcpStream, UdpSocket};

/// A Tokio aware source.
///
/// For more details, read the module level documentation.
pub trait Readiness {
    /// Returns true if the value is currently readable.
    fn is_readable(&self) -> bool;

    /// Returns true if the value is currently writable.
    fn is_writable(&self) -> bool;
}

const READABLE: usize = 1;
const WRITABLE: usize = 1 << 2;

/// Represents the state of readiness.
///
/// A given source may be ready to complete zero, one, or more operations.
/// `Ready` represents which operations the source is ready to complete.
///
/// Currently, the operations are reading and writing.
#[derive(Copy, PartialEq, Eq, Clone, PartialOrd, Ord)]
pub struct Ready(usize);

impl Ready {
    /// No readiness
    pub fn none() -> Ready {
        Ready(0)
    }

    /// Read and write readiness
    pub fn all() -> Ready {
        Ready::readable() | Ready::writable()
    }

    /// Read readiness
    pub fn readable() -> Ready {
        Ready(READABLE)
    }

    /// Write readiness
    pub fn writable() -> Ready {
        Ready(WRITABLE)
    }

    /// Returns true when read readiness is included
    pub fn is_readable(&self) -> bool {
        self.contains(Ready::readable())
    }

    /// Returns true when write readiness is included
    pub fn is_writable(&self) -> bool {
        self.contains(Ready::writable())
    }

    /// Returns true when the specified readiness is included
    pub fn contains(&self, other: Ready) -> bool {
        (*self & other) == other
    }
}

impl ops::BitOr for Ready {
    type Output = Ready;

    #[inline]
    fn bitor(self, other: Ready) -> Ready {
        Ready(self.0 | other.0)
    }
}

impl ops::BitXor for Ready {
    type Output = Ready;

    #[inline]
    fn bitxor(self, other: Ready) -> Ready {
        Ready(self.0 ^ other.0)
    }
}

impl ops::BitAnd for Ready {
    type Output = Ready;

    #[inline]
    fn bitand(self, other: Ready) -> Ready {
        Ready(self.0 & other.0)
    }
}

impl ops::Sub for Ready {
    type Output = Ready;

    #[inline]
    fn sub(self, other: Ready) -> Ready {
        Ready(self.0 & !other.0)
    }
}

impl ops::Not for Ready {
    type Output = Ready;

    #[inline]
    fn not(self) -> Ready {
        Ready(!self.0 & Ready::all().0)
    }
}

impl fmt::Debug for Ready {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        let mut one = false;
        let flags = [
            (Ready::readable(), "Readable"),
            (Ready::writable(), "Writable"),
            ];

        try!(write!(fmt, "Ready {{"));

        for &(flag, msg) in &flags {
            if self.contains(flag) {
                if one { try!(write!(fmt, " | ")) }
                try!(write!(fmt, "{}", msg));

                one = true
            }
        }

        try!(write!(fmt, "}}"));

        Ok(())
    }
}

impl Readiness for TcpStream {
    fn is_readable(&self) -> bool {
        self.poll_read().is_ready()
    }

    fn is_writable(&self) -> bool {
        self.poll_write().is_ready()
    }
}

impl Readiness for UdpSocket {
    fn is_readable(&self) -> bool {
        self.poll_read().is_ready()
    }

    fn is_writable(&self) -> bool {
        self.poll_write().is_ready()
    }
}
