/// A pipelined protocol frame
#[derive(Debug, Clone)]
pub enum Frame<T, B, E> {
    /// Either a request or a response
    Message {
        /// The message value
        message: T,
        /// Set to true when body frames will follow
        body: bool,
    },
    /// Body frame. None indicates that the body is done streaming.
    Body {
        /// Body chunk. Setting to `None` indicates that the body is done
        /// streaming and there will be no further body frames sent with the
        /// given request ID.
        chunk: Option<B>,
    },
    /// Error
    Error {
        /// Error value
        error: E,
    },
}

impl<T, B, E> Frame<T, B, E> {
    /// Unwraps a frame, yielding the content of the `Message`.
    pub fn unwrap_msg(self) -> T {
        match self {
            Frame::Message { message, .. } => message,
            Frame::Body { .. } => panic!("called `Frame::unwrap_msg()` on a `Body` value"),
            Frame::Error { .. } => panic!("called `Frame::unwrap_msg()` on an `Error` value"),
        }
    }

    /// Unwraps a frame, yielding the content of the `Body`.
    pub fn unwrap_body(self) -> Option<B> {
        match self {
            Frame::Body { chunk } => chunk,
            Frame::Message { .. } => panic!("called `Frame::unwrap_body()` on a `Message` value"),
            Frame::Error { .. } => panic!("called `Frame::unwrap_body()` on an `Error` value"),
        }
    }

    /// Unwraps a frame, yielding the content of the `Error`.
    pub fn unwrap_err(self) -> E {
        match self {
            Frame::Error { error } => error,
            Frame::Body { .. } => panic!("called `Frame::unwrap_err()` on a `Body` value"),
            Frame::Message { .. } => panic!("called `Frame::unwrap_err()` on a `Message` value"),
        }
    }
}
