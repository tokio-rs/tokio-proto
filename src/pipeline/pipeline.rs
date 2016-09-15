use super::{Error, Frame, Message, Transport};
use futures::stream::{Stream, Sender, FutureSender};
use futures::{Future, Poll, Async};
use std::io;

// TODO:
//
// - Wait for service readiness
// - Handle request body stream cancellation

/// Provides protocol pipelining functionality in a generic way over clients
/// and servers. Used internally by `pipeline::Client` and `pipeline::Server`.
pub struct Pipeline<S, T>
    where T: Transport,
          S: Dispatch,
{
    // True as long as the connection has more request frames to read.
    run: bool,
    // The transport wrapping the connection.
    transport: T,
    // The `Sender` for the current request body stream
    out_body: Option<BodySender<T::BodyOut, S::Error>>,
    // The response body stream
    in_body: Option<S::InBodyStream>,
    // True when the transport is fully flushed
    is_flushed: bool,
    // Glues the service with the pipeline task
    dispatch: S,
}

/// Dispatch messages from the transport to the service
pub trait Dispatch {
    /// Message written to transport
    type InMsg;

    /// Body written to transport
    type InBody;

    /// Body stream written to transport
    type InBodyStream: Stream<Item = Self::InBody, Error = Self::Error>;

    type OutMsg;

    type Error;

    /// Process an out message
    fn dispatch(&mut self, message: Self::OutMsg) -> io::Result<()>;

    /// Poll the next completed message
    fn poll(&mut self) -> Option<Result<Message<Self::InMsg, Self::InBodyStream>, Self::Error>>;

    /// RPC currently in flight
    fn has_in_flight(&self) -> bool;
}

enum BodySender<B, E> {
    Ready(Sender<B, E>),
    Busy(FutureSender<B, E>),
}

impl<S, T, E> Pipeline<S, T>
    where T: Transport<Error = E>,
          S: Dispatch<InMsg = T::In, InBody = T::BodyIn, OutMsg = T::Out, Error = E>,
          E: From<Error<E>>,
{
    /// Create a new pipeline `Pipeline` dispatcher with the given service and
    /// transport
    pub fn new(dispatch: S, transport: T) -> io::Result<Pipeline<S, T>> {
        Ok(Pipeline {
            run: true,
            transport: transport,
            out_body: None,
            in_body: None,
            is_flushed: true,
            dispatch: dispatch,
        })
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

            if let Async::Ready(frame) = try!(self.transport.read()) {
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

    fn process_out_frame(&mut self, frame: Frame<T::Out, T::BodyOut, E>) -> io::Result<()> {
        trace!("process_out_frame");
        // At this point, the service & transport are ready to process the
        // frame, no matter what it is.
        match frame {
            Frame::Message(out_message) => {
                trace!("read out message");
                // There is no streaming body. Set `out_body` to `None` so that
                // the previous body stream is dropped.
                self.out_body = None;

                if let Err(_) = self.dispatch.dispatch(out_message) {
                    // TODO: Should dispatch be infalliable
                    unimplemented!();
                }
            }
            Frame::MessageWithBody(out_message, body_sender) => {
                trace!("read out message with body");
                // Track the out body sender. If `self.out_body`
                // currently holds a sender for the previous out body, it
                // will get dropped. This terminates the stream.
                self.out_body = Some(BodySender::Ready(body_sender));

                if let Err(_) = self.dispatch.dispatch(out_message) {
                    // TODO: Should dispatch be infallible
                    unimplemented!();
                }
            }
            Frame::Body(Some(chunk)) => {
                trace!("read out body chunk");
                try!(self.process_out_body_chunk(chunk));
            }
            Frame::Body(None) => {
                trace!("read out body EOF");
                // Drop the sender.
                // TODO: Ensure a sender exists
                let _ = self.out_body.take();
            }
            Frame::Done => {
                trace!("read Frame::Done");
                // At this point, we just return. This works
                // because tick() will be called again and go
                // through the read-cycle again.
                self.run = false;
            }
            Frame::Error(_) => {
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
        while self.transport.poll_write().is_ready() {
            // Ensure the current in body is fully written
            if !try!(self.write_in_body()) {
                debug!("write in body not done");
                break;
            }
            debug!("write in body done");

            // Write the next in-flight in message
            if let Some(resp) = self.dispatch.poll() {
                try!(self.write_in_message(resp));
            } else {
                break;
            }
        }

        Ok(())
    }

    fn write_in_message(&mut self, message: Result<Message<S::InMsg, S::InBodyStream>, S::Error>) -> io::Result<()> {
        trace!("write_in_message");
        match message {
            Ok(Message::WithoutBody(val)) => {
                trace!("got in_flight value without body");
                try!(self.transport.write(Frame::Message(val)));

                // TODO: don't panic maybe if this isn't true?
                assert!(self.in_body.is_none());

                // Track the response body
                self.in_body = None;
            }
            Ok(Message::WithBody(val, body)) => {
                trace!("got in_flight value with body");
                try!(self.transport.write(Frame::Message(val)));

                // TODO: don't panic maybe if this isn't true?
                assert!(self.in_body.is_none());

                // Track the response body
                self.in_body = Some(body);
            }
            Err(e) => {
                trace!("got in_flight error");
                try!(self.transport.write(Frame::Error(e)));
            }
        }

        Ok(())
    }

    // Returns true if the response body is fully written
    fn write_in_body(&mut self) -> io::Result<bool> {
        trace!("write_in_body");
        if let Some(ref mut body) = self.in_body {
            while self.transport.poll_write().is_ready() {
                match body.poll() {
                    Ok(Async::Ready(Some(chunk))) => {
                        let r = try!(self.transport.write(Frame::Body(Some(chunk))));
                        if !r.is_ready() {
                            return Ok(false);
                        }
                    }
                    Ok(Async::Ready(None)) => {
                        try!(self.transport.write(Frame::Body(None)));
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
        self.is_flushed = try!(self.transport.flush()).is_ready();
        Ok(())
    }
}

impl<S, T, E> Future for Pipeline<S, T>
    where T: Transport<Error = E>,
          S: Dispatch<InMsg = T::In, InBody = T::BodyIn, OutMsg = T::Out, Error = E>,
          E: From<Error<E>>,
{
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
