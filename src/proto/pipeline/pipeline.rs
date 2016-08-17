use super::{Error, Frame, OutFrame, Transport};
use reactor::{Task, Tick};
use util::future::{Await, AwaitStream, Sender, BusySender};
use futures::stream::Stream;
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
    in_body: Option<AwaitStream<S::InBodyStream>>,
    // True when the transport is fully flushed
    is_flushed: bool,
    // Glues the service with the pipeline task
    dispatch: S,
}

/// Dispatch messages from the transport to the service
pub trait Dispatch {
    /// Message written to transport
    type InMsg: Send + 'static;

    /// Body written to transport
    type InBody: Send + 'static;

    /// Body stream written to transport
    type InBodyStream: Stream<Item = Self::InBody, Error = Self::Error>;

    type OutMsg: Send + 'static;

    type Error: Send + 'static;

    /// Process an out message
    fn dispatch(&mut self, message: Self::OutMsg) -> io::Result<()>;

    fn poll(&mut self) -> Option<Result<(Self::InMsg, Option<Self::InBodyStream>), Self::Error>>;

    /// RPC currently in flight
    fn has_in_flight(&self) -> bool;
}

enum BodySender<B, E>
    where B: Send + 'static,
          E: Send + 'static,
{
    Ready(Sender<B, E>),
    Busy(Await<BusySender<B, E>>),
}

impl<S, T, E> Pipeline<S, T>
    where T: Transport<Error = E>,
          S: Dispatch<InMsg = T::In, InBody = T::BodyIn, OutMsg = T::Out, Error = E>,
          E: From<Error<E>> + Send + 'static,
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

    // Tick the pipeline state machine
    pub fn tick(&mut self) -> io::Result<Tick> {
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
            return Ok(Tick::Final);
        }

        // Tick again later
        Ok(Tick::WouldBlock)
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

            if let Some(frame) = try!(self.transport.read()) {
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
                // The body sender is ready
                return true;
            }
            Some(BodySender::Busy(ref mut busy)) => {
                match busy.poll() {
                    Some(Ok(sender)) => sender,
                    Some(Err(_)) => unimplemented!(),
                    None => {
                        // Not ready
                        return false;
                    }
                }
            }
            None => return true,
        };

        self.out_body = Some(BodySender::Ready(sender));
        true
    }

    fn process_out_frame(&mut self, frame: OutFrame<T::Out, E, T::BodyOut>) -> io::Result<()> {
        // At this point, the service & transport are ready to process the
        // frame, no matter what it is.
        match frame {
            Frame::Message((out_message, body_sender)) => {
                trace!("read out message");
                // Track the out body sender. If `self.out_body`
                // currently holds a sender for the previous out body, it
                // will get dropped. This terminates the stream.
                self.out_body = body_sender.map(BodySender::Ready);

                if let Err(_) = self.dispatch.dispatch(out_message) {
                    // TODO: Should dispatch be infalliable
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
        match self.out_body.take() {
            Some(BodySender::Ready(sender)) => {
                // Try sending the out body chunk
                let busy = match sender.send(Ok(chunk)) {
                    Ok(busy) => busy,
                    Err(_) => {
                        // The rx half is gone. There is no longer any interest
                        // in the out body.
                        return Ok(());
                    }
                };

                let await = try!(Await::new(busy));
                self.out_body = Some(BodySender::Busy(await));
            }
            Some(BodySender::Busy(..)) => {
                // This case should never happen but it may be better to fail a
                // bit more gracefully in the event of an internal bug than to
                // panic.
                unimplemented!();
            }
            None => {
                // The rx half canceled interest, there is nothing else to do
            }
        }

        Ok(())
    }

    fn write_in_frames(&mut self) -> io::Result<()> {
        while self.transport.is_writable() {
            // Ensure the current in body is fully written
            if !try!(self.write_in_body()) {
                break;
            }

            // Write the next in-flight in message
            if let Some(resp) = self.dispatch.poll() {
                try!(self.write_in_message(resp));
            } else {
                break;
            }
        }

        Ok(())
    }

    fn write_in_message(&mut self, message: Result<(S::InMsg, Option<S::InBodyStream>), S::Error>) -> io::Result<()> {
        match message {
            Ok((val, body)) => {
                trace!("got in_flight value");
                try!(self.transport.write(Frame::Message(val)));

                // TODO: don't panic maybe if this isn't true?
                assert!(self.in_body.is_none());

                // Track the response body
                if let Some(body) = body {
                    self.in_body = Some(try!(AwaitStream::new(body)));
                }
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
        if let Some(ref mut body) = self.in_body {
            match body.poll() {
                Some(Ok(Some(chunk))) => {
                    try!(self.transport.write(Frame::Body(Some(chunk))));
                    return Ok(false);
                }
                Some(Ok(None)) => {
                    try!(self.transport.write(Frame::Body(None)));
                    // Response body flushed, let fall through
                }
                Some(Err(_)) => {
                    unimplemented!();
                }
                None => {
                    return Ok(false);
                }
            }
        }

        self.in_body = None;
        Ok(true)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.is_flushed = try!(self.transport.flush()).is_some();
        Ok(())
    }
}

impl<S, T, E> Task for Pipeline<S, T>
    where T: Transport<Error = E>,
          S: Dispatch<InMsg = T::In, InBody = T::BodyIn, OutMsg = T::Out, Error = E>,
          E: From<Error<E>> + Send + 'static,
{
    // Tick the pipeline state machine
    fn tick(&mut self) -> io::Result<Tick> {
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
            return Ok(Tick::Final);
        }

        // Tick again later
        Ok(Tick::WouldBlock)
    }
}
