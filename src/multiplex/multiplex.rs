use super::{Frame, Message, Error, RequestId, Transport};
use super::frame_buf::{FrameBuf, FrameDeque};
use futures::{Future, Poll, Async};
use futures::stream::{Stream};
use std::io;

/*
 * TODO:
 *
 * - Move constants to configuration settings
 *
 */

/// The max number of buffered frames that the connection can support. Once
/// this number is reached.
///
/// See module docs for more detail
const MAX_BUFFERED_FRAMES: usize = 128;

/// Provides protocol multiplexing functionality in a generic way over clients
/// and servers. Used internally by `multiplex::Client` and
/// `multiplex::Server`.
pub struct Multiplex<S, T>
    where T: Transport,
          S: Dispatch,
{
    // True as long as the connection has more request frames to read.
    run: bool,
    // The transport wrapping the connection.
    transport: T,
    // The `Sender` for the in-flight request body streams
    // out_bodies: HashMap<RequestId, BodySender<T::BodyOut, S::Error>>,
    // The in-flight response body streams
    // in_bodies: HashMap<RequestId, S::InBodyStream>,
    // True when the transport is fully flushed
    is_flushed: bool,
    // Glues the service with the pipeline task
    dispatch: S,
    // Buffer of pending messages for the dispatch
    dispatch_deque: FrameDeque<Frame<T::Out, T::BodyOut, S::Error>>,
    // Storage for buffered frames
    // frame_buf: FrameBuf<Frame<T::Out, T::BodyOut, S::Error>>,
    // Temporary storage for RequestIds...
    // scratch: Vec<RequestId>,
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
    fn dispatch(&mut self, request_id: RequestId, message: Self::OutMsg) -> io::Result<()>;

    /// Poll the next completed message
    fn poll(&mut self) -> Option<(RequestId, Result<Message<Self::InMsg, Self::InBodyStream>, Self::Error>)>;

    /// The `Dispatch` is ready to accept another message
    fn is_ready(&self) -> bool;

    /// RPC currently in flight
    fn has_in_flight(&self) -> bool;
}

/*
enum BodySender<B, E> {
    Ready(Sender<B, E>),
    Busy(FutureSender<B, E>, FrameDeque<Option<B>>),
}
*/

/*
 *
 * ===== impl Multiplex =====
 *
 */

impl<S, T, E> Multiplex<S, T>
    where T: Transport<Error = E>,
          S: Dispatch<InMsg = T::In, InBody = T::BodyIn, OutMsg = T::Out, Error = E>,
          E: From<Error<E>>,
{
    /// Create a new pipeline `Multiplex` dispatcher with the given service and
    /// transport
    pub fn new(dispatch: S, transport: T) -> io::Result<Multiplex<S, T>> {
        let frame_buf = FrameBuf::with_capacity(MAX_BUFFERED_FRAMES);

        Ok(Multiplex {
            run: true,
            transport: transport,
            // out_bodies: HashMap::new(),
            // in_bodies: HashMap::new(),
            is_flushed: true,
            dispatch: dispatch,
            dispatch_deque: frame_buf.deque(),
            // frame_buf: frame_buf,
            // scratch: vec![],
        })
    }

    /// Returns true if the multiplexer has nothing left to do
    fn is_done(&self) -> bool {
        !self.run && self.is_flushed && !self.dispatch.has_in_flight()
    }

    fn read_out_frames(&mut self) -> io::Result<()> {
        // try!(self.flush_out_bodies());

        while self.run {
            // TODO: Only read frames if there is available space in the frame
            // buffer
            if let Some(frame) = try!(self.transport.read()) {
                try!(self.process_out_frame(frame));
            } else {
                break;
            }
        }

        Ok(())
    }

    fn flush_dispatch_deque(&mut self) -> io::Result<()> {
        while self.dispatch.is_ready() {
            match self.dispatch_deque.pop() {
                Some(Frame::Message(request_id, msg)) => {
                    if let Err(_) = self.dispatch.dispatch(request_id, msg) {
                        unimplemented!();
                    }
                }
                Some(_) => unimplemented!(),
                None => return Ok(()),
            }
        }

        Ok(())
    }

    /*
    fn flush_out_bodies(&mut self) -> io::Result<()> {
        self.scratch.clear();

        for (request_id, body_sender) in self.out_bodies.iter_mut() {
            if body_sender.flush() {
                self.scratch.push(*request_id);
            }
        }

        // Purge the scratch
        for request_id in &self.scratch {
            self.out_bodies.remove(request_id);
        }

        Ok(())
    }
    */

    fn process_out_frame(&mut self, frame: Frame<T::Out, T::BodyOut, E>) -> io::Result<()> {
        trace!("Multiplex::process_out_frame");
        match frame {
            Frame::Message(id, out_message) => {
                trace!("   --> read out message; id={:?}", id);

                if self.dispatch.is_ready() {
                    trace!("   --> dispatch ready -- dispatching");

                    // Only should be here if there are no queued messages
                    assert!(self.dispatch_deque.is_empty());

                    if let Err(_) = self.dispatch.dispatch(id, out_message) {
                        // TODO: Should dispatch be infalliable
                        unimplemented!();
                    }
                } else {
                    trace!("   --> dispatch not ready");
                    // Queue the dispatch buffer
                    self.dispatch_deque.push(Frame::Message(id, out_message));
                }
            }
            Frame::MessageWithBody(_id, _out_message, _body_sender) => {
                unimplemented!();
            }
            Frame::Body(_id, Some(_chunk)) => {
                unimplemented!();
            }
            Frame::Body(_id, None) => {
                unimplemented!();
            }
            Frame::Done => {
                trace!("read Frame::Done");
                // At this point, we just return. This works
                // because tick() will be called again and go
                // through the read-cycle again.
                self.run = false;
            }
            Frame::Error(_, _) => {
                unimplemented!();
            }
        }

        Ok(())
    }

    fn write_in_frames(&mut self) -> io::Result<()> {
        while self.transport.poll_write().is_ready() {
            // Write the next in-flight in message
            match self.dispatch.poll() {
                Some((id, msg)) => try!(self.write_in_message(id, msg)),
                None => break,
            }
        }

        Ok(())
    }

    fn write_in_message(&mut self, id: RequestId, message: Result<Message<S::InMsg, S::InBodyStream>, S::Error>) -> io::Result<()> {
        match message {
            Ok(Message::WithoutBody(val)) => {
                trace!("got in_flight value with body");
                try!(self.transport.write(Frame::Message(id, val)));
            }
            Ok(Message::WithBody(_val, _body)) => {
                unimplemented!();
            }
            Err(e) => {
                trace!("got in_flight error");
                try!(self.transport.write(Frame::Error(id, e)));
            }
        }

        Ok(())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.is_flushed = try!(self.transport.flush()).is_some();
        Ok(())
    }
}

impl<S, T, E> Future for Multiplex<S, T>
    where T: Transport<Error = E>,
          S: Dispatch<InMsg = T::In, InBody = T::BodyIn, OutMsg = T::Out, Error = E>,
          E: From<Error<E>>,
{
    type Item = ();
    type Error = io::Error;

    // Tick the pipeline state machine
    fn poll(&mut self) -> Poll<(), io::Error> {
        trace!("Multiplex::tick");

        // Always flush the transport first
        try!(self.flush());

        // Next try to dispatch any buffered messages
        try!(self.flush_dispatch_deque());

        // First read off data from the socket
        try!(self.read_out_frames());

        // Handle completed responses
        try!(self.write_in_frames());

        // Since writing frames could un-block the dispatch, attempt to flush
        // the dispatch queue again.
        try!(self.flush_dispatch_deque());

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
            return Ok(Async::Ready(()));
        }

        // Tick again later
        Ok(Async::NotReady)
    }
}

/*
 *
 * ===== BusySender =====
 *
 */

/*
impl<B, E> BodySender<B, E> {
    fn flush(&mut self) -> bool {
        let ready_sender;

        // Attempt to flush frames as long as the sender is ready
        loop {
            if let BodySender::Busy(ref mut busy, ref mut frames) = *self {
                match busy.poll() {
                    Ok(Async::Ready(sender)) => {
                        // Now, if there is a pending frame to send.. send it
                        match frames.pop() {
                            Some(Some(chunk)) => {
                                let b = sender.send(Ok(chunk));
                                *busy = b;
                            }
                            Some(None) => {
                                // Done sending the body chunks, drop the
                                // sender
                                return true;
                            }
                            None => {
                                ready_sender = sender;
                                break;
                            }
                        }
                    }
                    Ok(Async::NotReady) => {},
                    Err(_) => {
                        // The receiving end dropped interest in the body
                        // stream. In this case, the sender and the frame
                        // buffer is dropped. If future body frames are
                        // received, the sender will be gone and the frames
                        // will be dropped.
                        return true;
                    }
                }
            } else {
                return false;
            }
        };

        *self = BodySender::Ready(ready_sender);
        false
    }
}
*/
