use std::{error, fmt, io};

/// Error returned as an Error frame or an `io::Error` that occurerred during
/// normal processing of the Transport
#[derive(Debug)]
pub enum Error<E> {
    /// Transport frame level error
    Transport(E),
    /// I/O level error
    Io(io::Error),
}

impl<E: fmt::Display> fmt::Display for Error<E> {
    fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
        match *self {
            Error::Transport(ref e) => fmt::Display::fmt(e, fmt),
            Error::Io(ref e) => fmt::Display::fmt(e, fmt),
        }
    }
}

impl<E: error::Error> error::Error for Error<E> {
    fn description(&self) -> &str {
        match *self {
            Error::Transport(ref e) => error::Error::description(e),
            Error::Io(ref e) => error::Error::description(e),
        }
    }

    fn cause(&self) -> Option<&error::Error> {
        match *self {
            Error::Transport(ref e) => error::Error::cause(e),
            Error::Io(ref e) => error::Error::cause(e),
        }
    }
}

impl From<Error<io::Error>> for io::Error {
    fn from(err: Error<io::Error>) -> Self {
        match err {
            Error::Transport(e) |
            Error::Io(e) => e,
        }
    }
}
