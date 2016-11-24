//! Provides the substrate for implementing pipelined, streaming protocols.
//!
//! In most cases, it's sufficient to work with `streaming::pipeline::{Client,
//! Server}` instead. But for some advanced protocols in which the client and
//! servers have more of a peer relationship, it's useful to work directly with
//! these implementation details.

use futures::sync::mpsc;
use futures::{Future, Poll, Async, Stream, Sink, AsyncSink, StartSend};
use std::io;
use streaming::{Message, Body};
use super::Frame;
use transport::Transport;

// TODO:
//
// - Wait for service readiness
// - Handle request body stream cancellation

/// Provides protocol pipelining functionality in a generic way over clients
/// and servers. Used internally by `pipeline::Client` and `pipeline::Server`.
pub struct Pipeline<T> where T: Dispatch {
    // True as long as the connection has more request frames to read.
    run: bool,

    // Glues the service with the pipeline task
    dispatch: T,

    in_body_next: Option<<T::Transport as Transport<T::Io>>::WriteFrame>,

    // The `Sender` for the current request body stream
    out_body: Option<BodySender<T::BodyOut, T::Error>>,

    // The response body stream
    in_body: Option<T::Stream>,

    // True when the transport is fully flushed
    is_flushed: bool,
}

/// Message used to communicate through the multiplex dispatch
pub type PipelineMessage<T, B, E> = Result<Message<T, B>, E>;

/// Dispatch messages from the transport to the service
pub trait Dispatch {
    /// Type of underlying I/O object
    type Io;

    /// Message written to transport
    type In;

    /// Body written to transport
    type BodyIn;

    /// Messages read from the transport
    type Out;

    /// Outbound body frame
    type BodyOut;

    /// Transport error
    type Error: From<io::Error>;

    /// Body stream written to transport
    type Stream: Stream<Item = Self::BodyIn, Error = Self::Error>;

    /// Transport type
    type Transport: Transport<Self::Io,
                              ReadFrame = Frame<Self::Out, Self::BodyOut, Self::Error>,
                              WriteFrame = Frame<Self::In, Self::BodyIn, Self::Error>>;

    /// Mutable reference to the transport
    fn transport(&mut self) -> &mut Self::Transport;

    /// Process an out message
    fn dispatch(&mut self, message: PipelineMessage<Self::Out, Body<Self::BodyOut, Self::Error>, Self::Error>) -> io::Result<()>;

    /// Poll the next completed message
    fn poll(&mut self) -> Poll<Option<PipelineMessage<Self::In, Self::Stream, Self::Error>>, io::Error>;

    /// RPC currently in flight
    /// TODO: Get rid of
    fn has_in_flight(&self) -> bool;
}

struct BodySender<B, E> {
    sender: mpsc::Sender<Result<B, E>>,
    queued: Option<Result<B, E>>,
}

struct MyTransport<'a, T: Dispatch + 'a> {
    inner: &'a mut Pipeline<T>,
}

impl<T> Pipeline<T> where T: Dispatch {
    /// Create a new pipeline `Pipeline` dispatcher with the given service and
    /// transport
    pub fn new(dispatch: T) -> Pipeline<T> {
        Pipeline {
            run: true,
            dispatch: dispatch,
            out_body: None,
            in_body: None,
            is_flushed: true,
            in_body_next: None,
        }
    }

    /// Returns true if the pipeline server dispatch has nothing left to do
    fn is_done(&self) -> bool {
        !self.run && self.is_flushed && !self.dispatch.has_in_flight()
    }

    fn read_out_frames(&mut self) -> io::Result<()> {
        while self.run {
            if !self.check_out_body_stream() {
                break;
            }

            if let Async::Ready(frame) = try!(self.dispatch.transport().poll()) {
                try!(self.process_out_frame(frame));
            } else {
                break;
            }
        }

        Ok(())
    }

    fn check_out_body_stream(&mut self) -> bool {
        let body = match self.out_body {
            Some(ref mut body) => body,
            None => return true,
        };

        if let Some(msg) = body.queued.take() {
            match body.sender.start_send(msg) {
                Ok(AsyncSink::Ready) => {}
                Ok(AsyncSink::NotReady(msg)) => {
                    body.queued = Some(msg);
                    return false
                }
                Err(_) => unimplemented!(),
            }
        }
        debug!("ready for a send");
        return true
    }

    fn process_out_frame(&mut self,
                         frame: Option<Frame<T::Out, T::BodyOut, T::Error>>)
                         -> io::Result<()> {
        trace!("process_out_frame");
        // At this point, the service & transport are ready to process the
        // frame, no matter what it is.
        match frame {
            Some(Frame::Message { message, body }) => {
                if body {
                    trace!("read out message with body");

                    let (tx, rx) = Body::pair();
                    let message = Message::WithBody(message, rx);

                    // Track the out body sender. If `self.out_body`
                    // currently holds a sender for the previous out body, it
                    // will get dropped. This terminates the stream.
                    self.out_body = Some(BodySender { sender: tx, queued: None });

                    if let Err(_) = self.dispatch.dispatch(Ok(message)) {
                        // TODO: Should dispatch be infallible
                        unimplemented!();
                    }
                } else {
                    trace!("read out message");

                    let message = Message::WithoutBody(message);

                    // There is no streaming body. Set `out_body` to `None` so that
                    // the previous body stream is dropped.
                    self.out_body = None;

                    if let Err(_) = self.dispatch.dispatch(Ok(message)) {
                        // TODO: Should dispatch be infalliable
                        unimplemented!();
                    }
                }
            }
            Some(Frame::Body { chunk }) => {
                match chunk {
                    Some(chunk) => {
                        trace!("read out body chunk");
                        try!(self.process_out_body_chunk(chunk));
                    }
                    None => {
                        trace!("read out body EOF");
                        // Drop the sender.
                        // TODO: Ensure a sender exists
                        let _ = self.out_body.take();
                    }
                }
            }
            None => {
                trace!("read Frame::Done");
                // At this point, we just return. This works
                // because tick() will be called again and go
                // through the read-cycle again.
                self.run = false;
            }
            Some(Frame::Error { .. }) => {
                // At this point, the transport is toast, there
                // isn't much else that we can do. Killing the task
                // will cause all in-flight requests to abort, but
                // they can't be written to the transport anyway...
                return Err(io::Error::new(io::ErrorKind::BrokenPipe, "An error occurred."));
            }
        }

        Ok(())
    }

    fn process_out_body_chunk(&mut self, chunk: T::BodyOut) -> io::Result<()> {
        trace!("process_out_body_chunk");
        let mut reset = false;
        match self.out_body {
            Some(ref mut body) => {
                debug!("sending a chunk");

                // Try sending the out body chunk
                match body.sender.start_send(Ok(chunk)) {
                    Ok(AsyncSink::Ready) => debug!("immediately done"),
                    Err(_e) => reset = true, // interest canceled
                    Ok(AsyncSink::NotReady(msg)) => {
                        debug!("not done yet");
                        if body.queued.is_some() {
                            // This case should never happen but it may be
                            // better to fail a bit more gracefully in the event
                            // of an internal bug than to panic.
                            unimplemented!();
                        }
                        body.queued = Some(msg);
                    }
                }
            }
            None => {
                debug!("interest canceled");
                // The rx half canceled interest, there is nothing else to do
            }
        }
        if reset {
            self.out_body = None;
        }
        Ok(())
    }

    fn write_in_frames(&mut self) -> io::Result<()> {
        trace!("write_in_frames");
        while try!(self.transport().poll_complete()).is_ready() {
            // Ensure the current in body is fully written
            if !try!(self.write_in_body()) {
                debug!("write in body not done");
                break;
            }
            debug!("write in body done");

            // Write the next in-flight in message
            match try!(self.dispatch.poll()) {
                Async::Ready(Some(Ok(message))) => {
                    trace!("   --> got message");
                    try!(self.write_in_message(Ok(message)));
                }
                Async::Ready(Some(Err(error))) => {
                    trace!("   --> got error");
                    try!(self.write_in_message(Err(error)));
                }
                Async::Ready(None) => {
                    trace!("   --> got None");
                    // The service is done with the connection. In this case, a
                    // `Done` frame should be written to the transport and the
                    // transport should start shutting down.
                    unimplemented!();
                }
                // Nothing to dispatch
                Async::NotReady => break,
            }
        }

        Ok(())
    }

    fn write_in_message(&mut self, message: Result<Message<T::In, T::Stream>, T::Error>) -> io::Result<()> {
        trace!("write_in_message");
        match message {
            Ok(Message::WithoutBody(val)) => {
                trace!("got in_flight value without body");
                let msg = Frame::Message { message: val, body: false };
                try!(assert_send(&mut self.transport(), msg));

                // TODO: don't panic maybe if this isn't true?
                assert!(self.in_body.is_none());

                // Track the response body
                self.in_body = None;
            }
            Ok(Message::WithBody(val, body)) => {
                trace!("got in_flight value with body");
                let msg = Frame::Message { message: val, body: true };
                try!(assert_send(&mut self.transport(), msg));

                // TODO: don't panic maybe if this isn't true?
                assert!(self.in_body.is_none());

                // Track the response body
                self.in_body = Some(body);
            }
            Err(e) => {
                trace!("got in_flight error");
                let msg = Frame::Error { error: e };
                try!(assert_send(&mut self.transport(), msg));
            }
        }

        Ok(())
    }

    // Returns true if the response body is fully written
    fn write_in_body(&mut self) -> io::Result<bool> {
        trace!("write_in_body");

        if self.in_body.is_some() {
            loop {
                if !try!(self.transport().poll_complete()).is_ready() {
                    return Ok(false);
                }

                match self.in_body.as_mut().unwrap().poll() {
                    Ok(Async::Ready(Some(chunk))) => {
                        try!(assert_send(&mut self.transport(),
                                         Frame::Body { chunk: Some(chunk) }));
                    }
                    Ok(Async::Ready(None)) => {
                        try!(assert_send(&mut self.transport(),
                                         Frame::Body { chunk: None }));
                        break;
                    }
                    Err(_) => {
                        unimplemented!();
                    }
                    Ok(Async::NotReady) => {
                        debug!("not ready");
                        return Ok(false);
                    }
                }
            }
        }

        self.in_body = None;
        Ok(true)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.is_flushed = try!(self.transport().poll_complete()).is_ready();
        Ok(())
    }

    fn transport(&mut self) -> MyTransport<T> {
        MyTransport { inner: self }
    }
}

fn assert_send<S: Sink>(s: &mut S, item: S::SinkItem) -> Result<(), S::SinkError> {
    match try!(s.start_send(item)) {
        AsyncSink::Ready => Ok(()),
        AsyncSink::NotReady(_) => {
            panic!("sink reported itself as ready after `poll_complete` but was \
                    then unable to accept a message")
        }
    }
}

impl<T> Future for Pipeline<T> where T: Dispatch {
    type Item = ();
    type Error = io::Error;

    // Tick the pipeline state machine
    fn poll(&mut self) -> Poll<(), io::Error> {
        trace!("Pipeline::tick");

        // Always flush the transport first
        try!(self.flush());

        // First read off data from the socket
        try!(self.read_out_frames());

        // Handle completed responses
        try!(self.write_in_frames());

        // Try flushing buffered writes
        try!(self.flush());

        // Clean shutdown of the pipeline server can happen when
        //
        // 1. The server is done running, this is signaled by Transport::read()
        //    returning Frame::Done.
        //
        // 2. The transport is done writing all data to the socket, this is
        //    signaled by Transport::flush() returning Ok(Some(())).
        //
        // 3. There are no further responses to write to the transport.
        //
        // It is necessary to perfom these three checks in order to handle the
        // case where the client shuts down half the socket.
        //
        if self.is_done() {
            return Ok(().into())
        }

        // Tick again later
        Ok(Async::NotReady)
    }
}

impl<'a, T: Dispatch> Sink for MyTransport<'a, T> {
    type SinkItem = <T::Transport as Transport<T::Io>>::WriteFrame;
    type SinkError = io::Error;

    fn start_send(&mut self, item: Self::SinkItem)
                  -> StartSend<Self::SinkItem, Self::SinkError>
    {
        if self.inner.in_body_next.is_some() {
            match try!(self.poll_complete()) {
                Async::Ready(()) => {}
                Async::NotReady => return Ok(AsyncSink::NotReady(item))
            }
        }
        assert!(self.inner.in_body_next.is_none());
        self.inner.in_body_next = Some(item);
        Ok(AsyncSink::Ready)
    }

    fn poll_complete(&mut self) -> Poll<(), Self::SinkError> {
        if let Some(next) = self.inner.in_body_next.take() {
            match try!(self.inner.dispatch.transport().start_send(next)) {
                AsyncSink::Ready => {}
                AsyncSink::NotReady(item) => {
                    self.inner.in_body_next = Some(item);
                    return Ok(Async::NotReady)
                }
            }
        }

        self.inner.dispatch.transport().poll_complete()
    }
}
