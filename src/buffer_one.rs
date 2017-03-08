#![allow(unused)]

use futures::{Poll, Async};
use futures::{StartSend, AsyncSink};
use futures::sink::Sink;
use futures::stream::Stream;
use futures::task::{self, Task};

use std::fmt;

#[must_use = "sinks do nothing unless polled"]
pub struct BufferOne<S: Sink> {
    sink: S,
    buf: Option<S::SinkItem>,
    task: Option<Task>,
}

impl<S: Sink> BufferOne<S> {
    pub fn new(sink: S) -> BufferOne<S> {
        BufferOne {
            sink: sink,
            buf: None,
            task: None,
        }
    }

    /// Get a shared reference to the inner sink.
    pub fn get_ref(&self) -> &S {
        &self.sink
    }

    /// Get a mutable reference to the inner sink.
    pub fn get_mut(&mut self) -> &mut S {
        &mut self.sink
    }

    pub fn poll_ready(&mut self) -> Async<()> {
        if self.buf.is_none() {
            return Async::Ready(());
        }

        self.task = Some(task::park());
        Async::NotReady
    }

    fn try_empty_buffer(&mut self) -> Poll<(), S::SinkError> {
        if let Some(buf) = self.buf.take() {
            if let AsyncSink::NotReady(buf) = try!(self.sink.start_send(buf)) {
                self.buf = Some(buf);
                return Ok(Async::NotReady);
            }

            // Unpark any pending tasks
            self.task.take()
                .map(|t| t.unpark());
        }

        Ok(Async::Ready(()))
    }
}

// Forwarding impl of Stream from the underlying sink
impl<S> Stream for BufferOne<S> where S: Sink + Stream {
    type Item = S::Item;
    type Error = S::Error;

    fn poll(&mut self) -> Poll<Option<S::Item>, S::Error> {
        self.sink.poll()
    }
}

impl<S: Sink> Sink for BufferOne<S> {
    type SinkItem = S::SinkItem;
    type SinkError = S::SinkError;

    fn start_send(&mut self, item: Self::SinkItem) -> StartSend<Self::SinkItem, Self::SinkError> {
        if !try!(self.try_empty_buffer()).is_ready() {
            return Ok(AsyncSink::NotReady(item));
        }

        if let AsyncSink::NotReady(item) = try!(self.sink.start_send(item)) {
            self.buf = Some(item);
        }

        Ok(AsyncSink::Ready)
    }

    fn poll_complete(&mut self) -> Poll<(), Self::SinkError> {
        let mut flushed = try!(self.try_empty_buffer()).is_ready();
        flushed &= try!(self.sink.poll_complete()).is_ready();

        if flushed {
            Ok(Async::Ready(()))
        } else {
            Ok(Async::NotReady)
        }
    }

    fn close(&mut self) -> Poll<(), Self::SinkError> {
        try_ready!(self.poll_complete());
        self.sink.close()
    }
}

impl<S> fmt::Debug for BufferOne<S>
    where S: Sink + fmt::Debug,
          S::SinkItem: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("BufferOne")
            .field("sink", &self.sink)
            .field("buf", &self.buf)
            .field("task", &self.task)
            .finish()
    }
}
