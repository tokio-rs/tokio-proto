use {Error, Message};
use super::{Frame, RequestId, Transport};
use super::frame_buf::{FrameBuf, FrameDeque};
use sender::Sender;
use futures::{Future, Poll, Async};
use futures::stream::Stream;
use std::io;
use std::collections::HashMap;

/*
 * TODO:
 *
 * - [BUG] Can only poll from body sender FutureSender in `flush`
 * - Move constants to configuration settings
 *
 */

/// The max number of buffered frames that the connection can support. Once
/// this number is reached.
///
/// See module docs for more detail
const MAX_BUFFERED_FRAMES: usize = 128;

/// Task that drives multiplexed protocols
///
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
    out_bodies: HashMap<RequestId, BodySender<T::Out, T::BodyOut, S::Error>>,
    // The in-flight response body streams
    in_bodies: HashMap<RequestId, S::InBodyStream>,
    // True when the transport is fully flushed
    is_flushed: bool,
    // Glues the service with the pipeline task
    dispatch: S,
    // Buffer of pending messages for the dispatch
    dispatch_deque: FrameDeque<Frame<T::Out, T::BodyOut, S::Error>>,
    // Storage for buffered frames
    frame_buf: FrameBuf<Frame<T::Out, T::BodyOut, S::Error>>,
    // Temporary storage for RequestIds...
    scratch: Vec<RequestId>,
}

/// Dispatch messages from the transport to the service
pub trait Dispatch {
    /// Message written to transport
    type InMsg;

    /// Body written to transport
    type InBody;

    /// Body stream written to transport
    type InBodyStream: Stream<Item = Self::InBody, Error = Self::Error>;

    /// Message read from the transprort
    type OutMsg;

    /// Error
    type Error;

    /// Process an out message
    fn dispatch(&mut self, request_id: RequestId, message: Result<Self::OutMsg, Self::Error>) -> io::Result<()>;

    /// Poll the next completed message
    fn poll(&mut self) -> Option<(RequestId, Result<Message<Self::InMsg, Self::InBodyStream>, Self::Error>)>;

    /// The `Dispatch` is ready to accept another message
    fn is_ready(&self) -> bool;

    /// RPC currently in flight
    fn has_in_flight(&self) -> bool;
}

struct BodySender<T, B, E> {
    deque: FrameDeque<Frame<T, B, E>>,
    sender: Sender<B, E>,
    // Tracks if the sender is ready. This value is computed on each tick when
    // the senders are flushed and before new frames are read.
    is_ready: bool,
}

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
    pub fn new(dispatch: S, transport: T) -> Multiplex<S, T> {
        let frame_buf = FrameBuf::with_capacity(MAX_BUFFERED_FRAMES);

        Multiplex {
            run: true,
            transport: transport,
            out_bodies: HashMap::new(),
            in_bodies: HashMap::new(),
            is_flushed: true,
            dispatch: dispatch,
            dispatch_deque: frame_buf.deque(),
            frame_buf: frame_buf,
            scratch: vec![],
        }
    }

    /// Returns true if the multiplexer has nothing left to do
    fn is_done(&self) -> bool {
        !self.run && self.is_flushed && !self.dispatch.has_in_flight() && self.out_bodies.len() == 0
    }

    fn read_out_frames(&mut self) -> io::Result<()> {
        try!(self.flush_out_bodies());

        while self.run {
            // TODO: Only read frames if there is available space in the frame
            // buffer
            if let Async::Ready(frame) = try!(self.transport.read()) {
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
                    try!(self.dispatch.dispatch(request_id, Ok(msg)));
                }
                Some(Frame::Error(request_id, err)) => {
                    try!(self.dispatch.dispatch(request_id, Err(err)));
                }
                Some(_) => panic!("unexpected dispatch frame"),
                None => return Ok(()),
            }
        }

        Ok(())
    }

    fn flush_out_bodies(&mut self) -> io::Result<()> {
        trace!("flush out bodies");

        self.scratch.clear();

        for (id, body_sender) in self.out_bodies.iter_mut() {
            body_sender.is_ready = true;

            trace!("   --> request={}", id);

            loop {
                match body_sender.sender.poll_ready() {
                    Ok(Async::Ready(())) => {
                        trace!("   --> ready");

                        // Pop a pending frame
                        match body_sender.deque.pop() {
                            Some(Frame::Body(_, Some(chunk))) => {
                                trace!("   --> sending chunk");
                                // Send the chunk
                                body_sender.sender.send(Ok(chunk));
                            }
                            Some(Frame::Body(_, None)) => {
                                trace!("   --> done, dropping");
                                // Done sending the body chunks, drop the sender
                                self.scratch.push(*id);
                                break;
                            }
                            Some(Frame::Error(_, err)) => {
                                trace!("   --> sending error");
                                // Send the error
                                body_sender.sender.send(Err(err));
                            }
                            Some(_) => unreachable!(),
                            None => {
                                trace!("   --> no queued frames");
                                // No more frames to flush
                                break;
                            }
                        }
                    }
                    Ok(Async::NotReady) => {
                        trace!("   --> not ready");
                        // Sender not ready
                        body_sender.is_ready = false;
                        break;
                    }
                    Err(_) => {
                        // The receiving end dropped interest in the body
                        // stream. In this case, the sender and the frame
                        // buffer is dropped. If future body frames are
                        // received, the sender will be gone and the frames
                        // will be dropped.
                        self.scratch.push(*id);
                        break;
                    }
                }
            }
        }

        // Purge the scratch
        for request_id in &self.scratch {
            trace!("drop out body handle; id={}", request_id);
            self.out_bodies.remove(request_id);
        }

        Ok(())
    }

    fn process_out_frame(&mut self, frame: Frame<T::Out, T::BodyOut, E>) -> io::Result<()> {
        trace!("Multiplex::process_out_frame");
        match frame {
            Frame::Message(id, out_message) => {
                trace!("   --> read out message; id={:?}", id);
                try!(self.process_out_msg(id, Ok(out_message)));
            }
            Frame::MessageWithBody(id, out_message, body_sender) => {
                trace!("   --> read out message; id={:?}", id);

                let body_sender = BodySender {
                    deque: self.frame_buf.deque(),
                    sender: body_sender.into(),
                    is_ready: true,
                };

                // Store the sender
                self.out_bodies.insert(id, body_sender);

                // Process the actual message
                try!(self.process_out_msg(id, Ok(out_message)));
            }
            Frame::Body(id, chunk) => {
                trace!("   --> read out body chunk");
                self.process_out_body_chunk(id, Ok(chunk));
            }
            Frame::Done => {
                trace!("read Frame::Done");
                assert!(self.in_bodies.len() == 0, "there are still in-bodies to process");
                self.run = false;
            }
            Frame::Error(id, err) => {
                try!(self.process_out_err(id, err));
            }
        }

        Ok(())
    }

    fn process_out_err(&mut self, id: RequestId, err: T::Error) -> io::Result<()> {
        trace!("   --> process error frame");
        if self.out_bodies.contains_key(&id) {
            trace!("   --> send error to body stream");
            self.process_out_body_chunk(id, Err(err));
            Ok(())
        } else {
            trace!("   --> send error to dispatcher");
            self.process_out_msg(id, Err(err))
        }
    }

    fn process_out_msg(&mut self, id: RequestId, msg: Result<T::Out, T::Error>) -> io::Result<()> {
        if self.dispatch.is_ready() {
            trace!("   --> dispatch ready -- dispatching");

            // Only should be here if there are no queued messages
            assert!(self.dispatch_deque.is_empty());

            if let Err(_) = self.dispatch.dispatch(id, msg) {
                // TODO: Should dispatch be infalliable
                unimplemented!();
            }
        } else {
            trace!("   --> dispatch not ready");

            // Queue the dispatch buffer
            let frame = match msg {
                Ok(msg) => Frame::Message(id, msg),
                Err(e) => Frame::Error(id, e),
            };

            self.dispatch_deque.push(frame);
        }

        Ok(())
    }

    fn process_out_body_chunk(&mut self, id: RequestId, chunk: Result<Option<T::BodyOut>, T::Error>) {
        trace!("process out body chunk; id={:?}", id);

        match self.out_bodies.get_mut(&id) {
            Some(body_sender) => {
                if body_sender.is_ready {
                    trace!("   --> sender is ready");

                    // Reverse Result & Option
                    let chunk = match chunk {
                        Ok(Some(v)) => Some(Ok(v)),
                        Ok(None) => None,
                        Err(e) => Some(Err(e)),
                    };

                    if let Some(chunk) = chunk {
                        trace!("   --> send chunk");

                        // Send the chunk
                        body_sender.sender.send(chunk);

                        // See if the sender is ready
                        match body_sender.sender.poll_ready() {
                            Ok(Async::Ready(_)) => {
                                trace!("   --> ready for more");
                                // The sender is ready for another message
                                return;
                            }
                            Ok(Async::NotReady) => {
                                // The sender is not ready for another message
                                body_sender.is_ready = false;
                                return;
                            }
                            Err(_) => {
                                // The sender has canceled interest, fall
                                // through and remove the body sender
                            }
                        }
                    }

                    trace!("   --> end of stream");

                    assert!(body_sender.deque.is_empty());
                    // Remote has dropped interest in the body. Fall through
                    // and remove the body sender handle.
                } else {
                    trace!("   --> queueing chunk");

                    let frame = match chunk {
                        Ok(chunk) => Frame::Body(id, chunk),
                        Err(e) => Frame::Error(id, e),
                    };

                    body_sender.deque.push(frame);
                    return;
                }
            }
            None => {
                debug!("interest in body canceled");
                return;
            }
        }

        trace!("dropping out body handle; id={:?}", id);
        self.out_bodies.remove(&id);
    }

    fn write_in_frames(&mut self) -> io::Result<()> {
        try!(self.write_in_messages());
        try!(self.write_in_body());
        Ok(())
    }

    fn write_in_messages(&mut self) -> io::Result<()> {
        trace!("write in messages");

        while self.transport.poll_write().is_ready() {
            trace!("   --> polling for in frame");

            if let Some((id, msg)) = self.dispatch.poll() {
                try!(self.write_in_message(id, msg));
            } else {
                break;
            }
        }

        trace!("   --> transport not ready");

        Ok(())
    }

    fn write_in_message(&mut self, id: RequestId, message: Result<Message<S::InMsg, S::InBodyStream>, S::Error>) -> io::Result<()> {
        match message {
            Ok(Message::WithoutBody(val)) => {
                trace!("got in_flight value without body");
                try!(self.transport.write(Frame::Message(id, val)));
            }
            Ok(Message::WithBody(val, body)) => {
                trace!("got in_flight value with body");

                // Write the in frame
                try!(self.transport.write(Frame::Message(id, val)));

                // Store the body stream for processing
                self.in_bodies.insert(id, body);
            }
            Err(e) => {
                trace!("got in_flight error");
                try!(self.transport.write(Frame::Error(id, e)));
            }
        }

        Ok(())
    }

    fn write_in_body(&mut self) -> io::Result<()> {
        trace!("write in body chunks; handlers={}", self.in_bodies.len());

        self.scratch.clear();

        // Now, write the ready streams
        'outer:
        for (id, body) in &mut self.in_bodies {
            trace!("   --> checking request {:?}", id);
            while self.transport.poll_write().is_ready() {
                match body.poll() {
                    Ok(Async::Ready(Some(chunk))) => {
                        trace!("   --> got chunk");

                        // TODO: handle return value
                        let frame = Frame::Body(*id, Some(chunk));
                        try!(self.transport.write(frame));
                    }
                    Ok(Async::Ready(None)) => {
                        trace!("   --> end of stream");

                        let frame = Frame::Body(*id, None);
                        try!(self.transport.write(frame));
                        // In body stream fully written, remove the entry and
                        // continue to the next body
                        self.scratch.push(*id);
                        continue 'outer;
                    }
                    Err(_) => {
                        unimplemented!();
                    }
                    Ok(Async::NotReady) => {
                        trace!("   --> no pending chunks");
                        continue 'outer;
                    }
                }
            }

            trace!("   --> transport not ready");

            // At this point, the transport is no longer ready, so no further
            // bodies can be processed
            break;
        }

        for id in &self.scratch {
            trace!("dropping in body handle; id={:?}", id);
            self.in_bodies.remove(id);
        }

        Ok(())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.is_flushed = try!(self.transport.flush()).is_ready();
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
        trace!("Multiplex::tick ~~~~~~~~~~~~~~~~~~~~~~~~~~~");

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
            trace!("multiplex done; terminating");
            return Ok(Async::Ready(()));
        }

        trace!("tick done; waiting for wake-up");

        // Tick again later
        Ok(Async::NotReady)
    }
}
