use {Error, Message, Body};
use super::{Frame, Transport};
use futures::stream::{Stream, Sender, FutureSender};
use futures::{Future, Poll, Async};
use std::io;

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
pub trait Dispatch: 'static {

    /// Message written to transport
    type In: 'static;

    /// Body written to transport
    type BodyIn: 'static;

    /// Messages read from the transport
    type Out: 'static;

    /// Outbound body frame
    type BodyOut: 'static;

    /// Transport error
    type Error: From<Error<Self::Error>> + 'static;

    /// Body stream written to transport
    type Stream: Stream<Item = Self::BodyIn, Error = Self::Error>;

    /// Transport type
    type Transport: Transport<In = Self::In,
                          BodyIn = Self::BodyIn,
                             Out = Self::Out,
                         BodyOut = Self::BodyOut,
                           Error = Self::Error>;

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

enum BodySender<B, E> {
    Ready(Sender<B, E>),
    Busy(FutureSender<B, E>),
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

            if let Async::Ready(frame) = try!(self.dispatch.transport().read()) {
                try!(self.process_out_frame(frame));
            } else {
                break;
            }
        }

        Ok(())
    }

    fn check_out_body_stream(&mut self) -> bool {
        let sender = match self.out_body {
            Some(BodySender::Ready(..)) => {
                debug!("ready for a send");
                // The body sender is ready
                return true;
            }
            Some(BodySender::Busy(ref mut busy)) => {
                debug!("waiting to be ready to send again");
                match busy.poll() {
                    Ok(Async::Ready(sender)) => sender,
                    Err(_) => unimplemented!(),
                    Ok(Async::NotReady) => {
                        // Not ready
                        return false;
                    }
                }
            }
            None => return true,
        };

        debug!("reading again for another send");
        self.out_body = Some(BodySender::Ready(sender));
        true
    }

    fn process_out_frame(&mut self, frame: Frame<T::Out, T::BodyOut, T::Error>) -> io::Result<()> {
        trace!("process_out_frame");
        // At this point, the service & transport are ready to process the
        // frame, no matter what it is.
        match frame {
            Frame::Message { message, body } => {
                if body {
                    trace!("read out message with body");

                    let (tx, rx) = Body::pair();
                    let message = Message::WithBody(message, rx);

                    // Track the out body sender. If `self.out_body`
                    // currently holds a sender for the previous out body, it
                    // will get dropped. This terminates the stream.
                    self.out_body = Some(BodySender::Ready(tx));

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
            Frame::Body { chunk } => {
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
            Frame::Done => {
                trace!("read Frame::Done");
                // At this point, we just return. This works
                // because tick() will be called again and go
                // through the read-cycle again.
                self.run = false;
            }
            Frame::Error { .. } => {
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
        match self.out_body.take() {
            Some(BodySender::Ready(sender)) => {
                debug!("sending a chunk");
                // Try sending the out body chunk
                let mut busy = sender.send(Ok(chunk));
                match busy.poll() {
                    Ok(Async::Ready(s)) => {
                        debug!("immediately done");
                        self.out_body = Some(BodySender::Ready(s));
                    }
                    Err(_e) => {} // interest canceled
                    Ok(Async::NotReady) => {
                        debug!("not done yet");
                        self.out_body = Some(BodySender::Busy(busy));
                    }
                }
                debug!("wut");
            }
            Some(BodySender::Busy(..)) => {
                // This case should never happen but it may be better to fail a
                // bit more gracefully in the event of an internal bug than to
                // panic.
                unimplemented!();
            }
            None => {
                debug!("interest canceled");
                // The rx half canceled interest, there is nothing else to do
            }
        }

        Ok(())
    }

    fn write_in_frames(&mut self) -> io::Result<()> {
        trace!("write_in_frames");
        while self.dispatch.transport().poll_write().is_ready() {
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
                try!(self.dispatch.transport().write(Frame::Message { message: val, body: false }));

                // TODO: don't panic maybe if this isn't true?
                assert!(self.in_body.is_none());

                // Track the response body
                self.in_body = None;
            }
            Ok(Message::WithBody(val, body)) => {
                trace!("got in_flight value with body");
                try!(self.dispatch.transport().write(Frame::Message { message: val, body: true }));

                // TODO: don't panic maybe if this isn't true?
                assert!(self.in_body.is_none());

                // Track the response body
                self.in_body = Some(body);
            }
            Err(e) => {
                trace!("got in_flight error");
                try!(self.dispatch.transport().write(Frame::Error { error: e }));
            }
        }

        Ok(())
    }

    // Returns true if the response body is fully written
    fn write_in_body(&mut self) -> io::Result<bool> {
        trace!("write_in_body");
        if let Some(ref mut body) = self.in_body {
            while self.dispatch.transport().poll_write().is_ready() {
                match body.poll() {
                    Ok(Async::Ready(Some(chunk))) => {
                        let r = try!(self.dispatch.transport().write(Frame::Body { chunk: Some(chunk) }));

                        // TODO: This doesn't seem correct anymore
                        if !r.is_ready() {
                            return Ok(false);
                        }
                    }
                    Ok(Async::Ready(None)) => {
                        try!(self.dispatch.transport().write(Frame::Body { chunk: None }));
                        // Response body flushed, let fall through
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
        self.is_flushed = try!(self.dispatch.transport().flush()).is_ready();
        Ok(())
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
