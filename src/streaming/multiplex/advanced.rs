//! Provides the substrate for implementing multiplexed, streaming protocols.
//!
//! In most cases, it's sufficient to work with `streaming::pipeline::{Client,
//! Server}` instead. But for some advanced protocols in which the client and
//! servers have more of a peer relationship, it's useful to work directly with
//! these implementation details.

use streaming::{Message, Body};
use futures::sync::mpsc;
use futures::{Future, Poll, Async, Stream, Sink, AsyncSink, StartSend};
use std::collections::hash_map::Entry;
use std::collections::{HashMap, VecDeque};
use std::{fmt, io};
use super::frame_buf::{FrameBuf, FrameDeque};
use super::{Frame, RequestId, Transport};
use buffer_one::BufferOne;

/*
 * TODO:
 *
 * - Handle errors correctly
 *    * When the FramedIo returns an error, how is it handled?
 *    * Is it sent to the dispatch?
 *    * Is it sent to the body?
 *    * What happens if there are in-flight *in* bodies
 *    * What happens if the out message is buffered?
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
pub struct Multiplex<T> where T: Dispatch {
    // True as long as the connection has more request frames to read.
    run: bool,

    // Used to track if any operations make progress
    made_progress: bool,

    // True when blocked on dispatch
    blocked_on_dispatch: bool,

    // True when blocked on flush
    blocked_on_flush: WriteState,

    // Glues the service with the pipeline task
    dispatch: BufferOne<DispatchSink<T>>,

    // Tracks in-progress exchanges
    exchanges: HashMap<RequestId, Exchange<T>>,

    // True when the transport is fully flushed
    is_flushed: bool,

    // RequestIds of exchanges that have not yet been dispatched
    dispatch_deque: VecDeque<RequestId>,

    // Storage for buffered frames
    frame_buf: FrameBuf<Option<Result<T::BodyOut, T::Error>>>,

    // Temporary storage for RequestIds...
    scratch: Vec<RequestId>,
}

impl<T> fmt::Debug for Multiplex<T>
    where T: Dispatch + fmt::Debug,
          T::In: fmt::Debug,
          T::Out: fmt::Debug,
          T::BodyIn: fmt::Debug,
          T::BodyOut: fmt::Debug,
          T::Error: fmt::Debug,
          T::Stream: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Multiplex")
            .field("run", &self.run)
            .field("made_progress", &self.made_progress)
            .field("blocked_on_dispatch", &self.blocked_on_dispatch)
            .field("dispatch", &self.dispatch)
            .field("exhanges", &self.exchanges)
            .field("is_flushed", &self.is_flushed)
            .field("dispatch_deque", &self.dispatch_deque)
            .field("frame_buf", &"FrameBuf { ... }")
            .field("scratch", &self.scratch)
            .finish()
    }
}

#[derive(Debug)]
struct DispatchSink<T> {
    inner: T,
}

type BodySender<B, E> = mpsc::Sender<Result<B, E>>;

/// Manages the state of a single in / out exchange
struct Exchange<T: Dispatch> {
    // Tracks the direction of the request as well as potentially buffers the
    // request message.
    //
    // The request message is only buffered when the dispatch is at capacity.
    // This case *shouldn't* happen and if it does it indicates a poorly
    // configured multiplex protocol or something a bit weird is happening.
    //
    // However, the world is full of multiplex protocols that don't have proper
    // flow control, so the case needs to be handled.
    request: Request<T>,

    // True indicates that the response has been handled
    responded: bool,

    // The outbound body stream sender
    out_body: Option<BodySender<T::BodyOut, T::Error>>,

    // Buffers outbound body chunks until the sender is ready
    out_deque: FrameDeque<Option<Result<T::BodyOut, T::Error>>>,

    // Tracks if the sender is ready. This value is computed on each tick when
    // the senders are flushed and before new frames are read.
    //
    // The reason readiness is tracked here is because if readiness changes
    // during the progress of the multiplex tick, an outbound body chunk can't
    // simply be dispatched. Order must be maintained, so any buffered outbound
    // chunks must be dispatched first.
    out_is_ready: bool,

    // The inbound body stream receiver
    in_body: Option<T::Stream>,
}

#[derive(Debug)]
enum Request<T: Dispatch> {
    In, // TODO: Handle inbound message buffering?
    Out(Option<Message<T::Out, Body<T::BodyOut, T::Error>>>),
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum WriteState {
    NoWrite,
    Wrote,
    Blocked,
}

/// Message used to communicate through the multiplex dispatch
#[derive(Debug)]
pub struct MultiplexMessage<T, B, E> {
    /// Request ID
    pub id: RequestId,
    /// Message
    pub message: Result<Message<T, B>, E>,
    /// True if message has no pair (request / response)
    pub solo: bool,
}

/// Dispatch messages from the transport to the service
pub trait Dispatch {
    /// Type of underlying I/O object
    type Io;

    /// Messages written to the transport
    type In;

    /// Inbound body frame
    type BodyIn;

    /// Messages read from the transport
    type Out;

    /// Outbound body frame
    type BodyOut;

    /// Transport error
    type Error: From<io::Error>;

    /// Inbound body stream type
    type Stream: Stream<Item = Self::BodyIn, Error = Self::Error>;

    /// Transport type
    type Transport: Transport<Self::BodyOut,
                              Item = Frame<Self::Out, Self::BodyOut, Self::Error>,
                              SinkItem = Frame<Self::In, Self::BodyIn, Self::Error>>;

    /// Mutable reference to the transport
    fn transport(&mut self) -> &mut Self::Transport;

    /// Poll the next available message
    fn poll(&mut self) -> Poll<Option<MultiplexMessage<Self::In, Self::Stream, Self::Error>>, io::Error>;

    /// The `Dispatch` is ready to accept another message
    fn poll_ready(&self) -> Async<()>;

    /// Process an out message
    fn dispatch(&mut self, message: MultiplexMessage<Self::Out, Body<Self::BodyOut, Self::Error>, Self::Error>) -> io::Result<()>;

    /// Cancel interest in the exchange identified by RequestId
    fn cancel(&mut self, request_id: RequestId) -> io::Result<()>;
}

/*
 *
 * ===== impl Multiplex =====
 *
 */

impl<T> Multiplex<T> where T: Dispatch {
    /// Create a new pipeline `Multiplex` dispatcher with the given service and
    /// transport
    pub fn new(dispatch: T) -> Multiplex<T> {
        // Add `Sink` impl for `Dispatch`
        let dispatch = DispatchSink { inner: dispatch };

        // Add a single slot buffer for the sink
        let dispatch = BufferOne::new(dispatch);

        let frame_buf = FrameBuf::with_capacity(MAX_BUFFERED_FRAMES);

        Multiplex {
            run: true,
            made_progress: false,
            blocked_on_dispatch: false,
            blocked_on_flush: WriteState::NoWrite,
            dispatch: dispatch,
            exchanges: HashMap::new(),
            is_flushed: true,
            dispatch_deque: VecDeque::new(),
            frame_buf: frame_buf,
            scratch: vec![],
        }
    }

    /// Returns true if the multiplexer has nothing left to do
    fn is_done(&self) -> bool {
        !self.run && self.is_flushed && self.exchanges.len() == 0
    }

    /// Attempt to dispatch any outbound request messages
    fn flush_dispatch_deque(&mut self) -> io::Result<()> {
        while self.dispatch.get_mut().inner.poll_ready().is_ready() {
            let id = match self.dispatch_deque.pop_front() {
                Some(id) => id,
                None => return Ok(()),
            };

            // Get the exchange
            let exchange = match self.exchanges.get_mut(&id) {
                Some(exchange) => exchange,
                None => continue,
            };

            if let Some(message) = exchange.take_buffered_out_request() {
                let message = MultiplexMessage {
                    id: id,
                    message: Ok(message),
                    solo: exchange.responded,
                };

                try!(self.dispatch.get_mut().inner.dispatch(message));
            }
        }

        // At this point, the task is blocked on the dispatcher
        self.blocked_on_dispatch = true;

        Ok(())
    }

    /// Dispatch any buffered outbound body frames to the sender
    fn flush_out_bodies(&mut self) -> io::Result<()> {
        trace!("flush out bodies");

        self.scratch.clear();

        for (id, exchange) in self.exchanges.iter_mut() {
            trace!("   --> request={}", id);
            try!(exchange.flush_out_body());

            // If the exchange is complete, track it for removal
            if exchange.is_complete() {
                self.scratch.push(*id);
            }
        }

        // Purge the scratch
        for id in &self.scratch {
            trace!("drop exchange; id={}", id);
            self.exchanges.remove(id);
        }

        Ok(())
    }

    /// Read and process frames from transport
    fn read_out_frames(&mut self) -> io::Result<()> {
        while self.run {
            // TODO: Only read frames if there is available space in the frame
            // buffer
            if let Async::Ready(frame) = try!(self.dispatch.get_mut().inner.transport().poll()) {
                try!(self.process_out_frame(frame));
            } else {
                break;
            }
        }

        Ok(())
    }

    /// Process outbound frame
    fn process_out_frame(&mut self,
                         frame: Option<Frame<T::Out, T::BodyOut, T::Error>>)
                         -> io::Result<()> {
        trace!("Multiplex::process_out_frame");

        match frame {
            Some(Frame::Message { id, message, body, solo }) => {
                if body {
                    let (tx, rx) = Body::pair();
                    let message = Message::WithBody(message, rx);

                    try!(self.process_out_message(id, message, Some(tx), solo));
                } else {
                    let message = Message::WithoutBody(message);

                    try!(self.process_out_message(id, message, None, solo));
                }
            }
            Some(Frame::Body { id, chunk }) => {
                trace!("   --> read out body chunk");
                self.process_out_body_chunk(id, Ok(chunk));
            }
            Some(Frame::Error { id, error }) => {
                try!(self.process_out_err(id, error));
            }
            None => {
                trace!("read None");
                // TODO: Ensure all bodies have been completed
                self.run = false;
            }
        }

        Ok(())
    }

    /// Process an outbound message
    fn process_out_message(&mut self,
                           id: RequestId,
                           message: Message<T::Out, Body<T::BodyOut, T::Error>>,
                           body: Option<mpsc::Sender<Result<T::BodyOut, T::Error>>>,
                           solo: bool)
                           -> io::Result<()>
    {
        trace!("   --> process message; body={:?}", body.is_some());

        match self.exchanges.entry(id) {
            Entry::Occupied(mut e) => {
                assert!(!e.get().responded, "invalid exchange state");
                assert!(e.get().is_inbound());

                // Dispatch the message. The dispatcher is not checked for
                // readiness in this case. This is because the message is a
                // response to a request initiated by the dispatch. It is
                // assumed that dispatcher can always process responses.
                try!(self.dispatch.get_mut().inner.dispatch(MultiplexMessage {
                    id: id,
                    message: Ok(message),
                    solo: solo,
                }));

                // Track that the exchange has been responded to
                e.get_mut().responded = true;

                // Set the body sender
                e.get_mut().out_body = body;

                // If the exchange is complete, clean up resources
                if e.get().is_complete() {
                    e.remove();
                }
            }
            Entry::Vacant(e) => {
                if self.dispatch.get_mut().inner.poll_ready().is_ready() {
                    trace!("   --> dispatch ready -- dispatching");

                    // Only should be here if there are no queued messages
                    assert!(self.dispatch_deque.is_empty());

                    // Create the exchange state
                    let mut exchange = Exchange::new(
                        Request::Out(None),
                        self.frame_buf.deque());

                    exchange.out_body = body;

                    // Set expect response
                    exchange.set_expect_response(solo);

                    if !exchange.is_complete() {
                        // Track the exchange
                        e.insert(exchange);
                    }

                    // Dispatch the message
                    try!(self.dispatch.get_mut().inner.dispatch(MultiplexMessage {
                        id: id,
                        message: Ok(message),
                        solo: solo,
                    }));
                } else {
                    trace!("   --> dispatch not ready");

                    self.blocked_on_dispatch = true;

                    // Create the exchange state, including the buffered message
                    let mut exchange = Exchange::new(
                        Request::Out(Some(message)),
                        self.frame_buf.deque());

                    exchange.out_body = body;

                    // Set expect response
                    exchange.set_expect_response(solo);

                    assert!(!exchange.is_complete());

                    // Track the exchange state
                    e.insert(exchange);

                    // Track the request ID as pending dispatch
                    self.dispatch_deque.push_back(id);
                }
            }
        }

        Ok(())
    }

    // Process an error
    fn process_out_err(&mut self, id: RequestId, err: T::Error) -> io::Result<()> {
        trace!("   --> process error frame");

        let mut remove = false;

        if let Some(exchange) = self.exchanges.get_mut(&id) {
            if !exchange.is_dispatched() {
                // The exchange is buffered and hasn't exited the multiplexer.
                // At this point it is safe to just drop the state
                remove = true;

                assert!(exchange.out_body.is_none());
                assert!(exchange.in_body.is_none());
            } else if exchange.is_outbound() {
                // Outbound exchanges can only have errors dispatched via the
                // body
                exchange.send_out_chunk(Err(err));

                // The downstream dispatch has not provided a response to the
                // exchange, indicate that interest has been canceled.
                if !exchange.responded {
                    try!(self.dispatch.get_mut().inner.cancel(id));
                }

                remove = exchange.is_complete();
            } else {
                if !exchange.responded {
                    // A response has not been provided yet, send the error via
                    // the dispatch
                    try!(self.dispatch.get_mut().inner.dispatch(MultiplexMessage::error(id, err)));

                    exchange.responded = true;
                } else {
                    // A response has already been sent, send the error via the
                    // body stream
                    exchange.send_out_chunk(Err(err));
                }

                remove = exchange.is_complete();
            }
        } else {
            trace!("   --> no in-flight exchange; dropping error");
        }

        if remove {
            self.exchanges.remove(&id);
        }

        Ok(())
    }

    fn process_out_body_chunk(&mut self, id: RequestId, chunk: Result<Option<T::BodyOut>, T::Error>) {
        trace!("process out body chunk; id={:?}", id);

        {
            let exchange = match self.exchanges.get_mut(&id) {
                Some(v) => v,
                _ => {
                    trace!("   --> exchange previously aborted; id={:?}", id);
                    return;
                }
            };

            exchange.send_out_chunk(chunk);

            if !exchange.is_complete() {
                return;
            }
        }

        trace!("dropping out body handle; id={:?}", id);
        self.exchanges.remove(&id);
    }

    fn write_in_frames(&mut self) -> io::Result<()> {
        try!(self.write_in_messages());
        try!(self.write_in_body());
        Ok(())
    }

    fn write_in_messages(&mut self) -> io::Result<()> {
        trace!("write in messages");

        while self.dispatch.poll_ready().is_ready() {
            trace!("   --> polling for in frame");

            match try!(self.dispatch.get_mut().inner.poll()) {
                Async::Ready(Some(message)) => {
                    self.dispatch_made_progress();

                    match message.message {
                        Ok(m) => {
                            trace!("   --> got message");
                            try!(self.write_in_message(message.id, m, message.solo));
                        }
                        Err(error) => {
                            trace!("   --> got error");
                            try!(self.write_in_error(message.id, error));
                        }
                    }
                }
                Async::Ready(None) => {
                    trace!("   --> got error");
                    trace!("   --> got None");
                    // The service is done with the connection. In this case, a
                    // `Done` frame should be written to the transport and the
                    // transport should start shutting down.
                    //
                    // However, the `Done` frame should only be written once
                    // all the in-flight bodies have been written.
                    //
                    // For now, do nothing...
                    break;
                }
                // Nothing to dispatch
                Async::NotReady => break,
            }
        }

        trace!("   --> transport not ready");
        self.blocked_on_flush.transport_not_write_ready();

        Ok(())
    }

    fn write_in_message(&mut self,
                        id: RequestId,
                        message: Message<T::In, T::Stream>,
                        solo: bool)
                        -> io::Result<()>
    {
        let (message, body) = match message {
            Message::WithBody(message, rx) => (message, Some(rx)),
            Message::WithoutBody(message) => (message, None),
        };

        // Create the frame
        let frame = Frame::Message {
            id: id,
            message: message,
            body: body.is_some(),
            solo: solo,
        };

        // Write the frame
        try!(assert_send(&mut self.dispatch, frame));
        self.blocked_on_flush.wrote_frame();

        match self.exchanges.entry(id) {
            Entry::Occupied(mut e) => {
                assert!(!e.get().responded, "invalid exchange state");
                assert!(e.get().is_outbound());
                assert!(!solo);

                // Track that the exchange has been responded to
                e.get_mut().responded = true;

                // Set the body receiver
                e.get_mut().in_body = body;

                // If the exchange is complete, clean up the resources
                if e.get().is_complete() {
                    e.remove();
                }
            }
            Entry::Vacant(e) => {
                // Create the exchange state
                let mut exchange = Exchange::new(
                    Request::In,
                    self.frame_buf.deque());

                // Set the body receiver
                exchange.in_body = body;
                exchange.set_expect_response(solo);

                if !exchange.is_complete() {
                    // Track the exchange
                    e.insert(exchange);
                }
            }
        }

        Ok(())
    }

    fn write_in_error(&mut self,
                      id: RequestId,
                      error: T::Error)
                      -> io::Result<()>
    {
        if let Entry::Occupied(mut e) = self.exchanges.entry(id) {
            assert!(!e.get().responded, "exchange already responded");

            // TODO: should the outbound body be canceled? In theory, if the
            // consuming end doesn't want it anymore, it should drop interest
            e.get_mut().responded = true;
            e.get_mut().out_body = None;
            e.get_mut().in_body = None;
            e.get_mut().out_deque.clear();

            assert!(e.get().is_complete());

            // Write the error frame
            let frame = Frame::Error { id: id, error: error };
            try!(assert_send(&mut self.dispatch, frame));
            self.blocked_on_flush.wrote_frame();

            e.remove();
        } else {
            trace!("exchange does not exist; id={:?}", id);
        }

        Ok(())
    }

    fn write_in_body(&mut self) -> io::Result<()> {
        trace!("write in body chunks");

        self.scratch.clear();

        // Now, write the ready streams
        'outer:
        for (&id, exchange) in &mut self.exchanges {
            trace!("   --> checking request {:?}", id);

            loop {
                if !try!(self.dispatch.poll_complete()).is_ready() {
                    trace!("   --> blocked on transport");
                    self.blocked_on_flush.transport_not_write_ready();
                    break 'outer;
                }

                match exchange.try_poll_in_body() {
                    Ok(Async::Ready(Some(chunk))) => {
                        trace!("   --> got chunk");

                        let frame = Frame::Body { id: id, chunk: Some(chunk) };
                        try!(assert_send(&mut self.dispatch, frame));
                        self.blocked_on_flush.wrote_frame();
                    }
                    Ok(Async::Ready(None)) => {
                        trace!("   --> end of stream");

                        let frame = Frame::Body { id: id, chunk: None };
                        try!(assert_send(&mut self.dispatch, frame));
                        self.blocked_on_flush.wrote_frame();

                        // in_body is fully written.
                        exchange.in_body = None;
                        break;
                    }
                    Err(error) => {
                        trace!("   --> got error");

                        // Write the error frame
                        let frame = Frame::Error { id: id, error: error };
                        try!(assert_send(&mut self.dispatch, frame));
                        self.blocked_on_flush.wrote_frame();

                        exchange.responded = true;
                        exchange.in_body = None;
                        exchange.out_body = None;
                        exchange.out_deque.clear();

                        debug_assert!(exchange.is_complete());
                        break;
                    }
                    Ok(Async::NotReady) => {
                        trace!("   --> no pending chunks");
                        continue 'outer;
                    }
                }
            }

            if exchange.is_complete() {
                self.scratch.push(id);
            }
        }

        for id in &self.scratch {
            trace!("dropping in body handle; id={:?}", id);
            self.exchanges.remove(id);
        }

        Ok(())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.is_flushed = try!(self.dispatch.poll_complete()).is_ready();

        // TODO: Technically, poll_complete needs to be called on the exchange body senders.
        // However, mpsc::Sender doesn't actually need to have poll_complete called as it is
        // currently a no-op. So, I'm just going to punt on figuring out the best way to handle
        // poll_complete.

        if self.is_flushed && self.blocked_on_flush == WriteState::Blocked {
            self.made_progress = true;
        }

        Ok(())
    }

    fn reset_flags(&mut self) {
        self.made_progress = false;
        self.blocked_on_dispatch = false;
        self.blocked_on_flush = WriteState::NoWrite;
    }

    fn dispatch_made_progress(&mut self) {
        if self.blocked_on_dispatch {
            self.made_progress = true;
        }
    }
}

impl<T> Future for Multiplex<T>
    where T: Dispatch,
{
    type Item = ();
    type Error = io::Error;

    // Tick the pipeline state machine
    fn poll(&mut self) -> Poll<(), io::Error> {
        trace!("Multiplex::tick ~~~~~~~~~~~~~~~~~~~~~~~~~~~");

        // Always tick the transport first
        self.dispatch.get_mut().inner.transport().tick();

        // Try to send any buffered body chunks on their senders
        //
        // This has to happen at the start of the tick. The sender readiness is computed for later
        // on.
        try!(self.flush_out_bodies());

        // Initially set the made_progress flag to true
        self.made_progress = true;

        // Keep looping as long as at least one operation succeeds
        while self.made_progress {
            trace!("~~ multiplex primary loop tick ~~");

            // Reset various flags tracking the state throughout this loop.
            self.reset_flags();

            // Try to dispatch any buffered messages
            try!(self.flush_dispatch_deque());

            // First read off data from the socket
            try!(self.read_out_frames());

            // Handle completed responses
            try!(self.write_in_frames());

            // Try flushing buffered writes
            try!(self.flush());
        }

        // Clean shutdown of the pipeline server can happen when
        //
        // 1. The server is done running, this is signaled by Transport::poll()
        //    returning None.
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

impl<T: Dispatch> Drop for Multiplex<T> {
    fn drop(&mut self) {
        if !self.exchanges.is_empty() {
            warn!("multiplexer dropping with in-flight exchanges");
        }
    }
}

impl<T: Dispatch> Exchange<T> {
    fn new(request: Request<T>, deque: FrameDeque<Option<Result<T::BodyOut, T::Error>>>) -> Exchange<T> {
        Exchange {
            request: request,
            responded: false,
            out_body: None,
            out_deque: deque,
            out_is_ready: true,
            in_body: None,
        }
    }

    fn is_inbound(&self) -> bool {
        match self.request {
            Request::In => true,
            Request::Out(_) => false,
        }
    }

    fn is_outbound(&self) -> bool {
        !self.is_inbound()
    }

    fn is_dispatched(&self) -> bool {
        match self.request {
            Request::Out(Some(_)) => false,
            _ => true,
        }
    }

    /// Returns true if the exchange is complete
    fn is_complete(&self) -> bool {
        // The exchange is completed if the response has been seen and bodies
        // in both directions are fully flushed
        self.responded &&
            self.out_body.is_none() &&
            self.in_body.is_none() &&
            self.request.is_none()
    }

    fn set_expect_response(&mut self, solo: bool) {
        self.responded = solo;

        if solo {
            if self.is_inbound() {
                assert!(self.out_body.is_none());
            } else {
                assert!(self.in_body.is_none());
            }
        }
    }

    /// Takes the buffered out request out of the value and returns it
    fn take_buffered_out_request(&mut self) -> Option<Message<T::Out, Body<T::BodyOut, T::Error>>> {
        match self.request {
            Request::Out(ref mut request) => request.take(),
            _ => None,
        }
    }

    fn send_out_chunk(&mut self, chunk: Result<Option<T::BodyOut>, T::Error>) {
        // Reverse Result & Option
        let chunk = match chunk {
            Ok(Some(v)) => Some(Ok(v)),
            Ok(None) => None,
            Err(e) => Some(Err(e)),
        };

        // Get a reference to the sender
        {
            let sender = match self.out_body {
                Some(ref mut v) => v,
                _ =>  {
                    return;
                }
            };

            if self.out_is_ready {
                trace!("   --> send chunk; end-of-stream={:?}", chunk.is_none());

                // If there is a chunk (vs. None which represents end of
                // stream)
                if let Some(chunk) = chunk {
                    match sender.start_send(chunk) {
                        Ok(AsyncSink::Ready) => {
                            trace!("   --> ready for more");
                            // The sender is ready for another message
                            return;
                        }
                        Ok(AsyncSink::NotReady(chunk)) => {
                            // The sender is not ready for another message
                            self.out_deque.push(Some(chunk));
                            self.out_is_ready = false;

                            return;
                        }
                        Err(_) => {
                            // The sender is complete, it should be removed
                        }
                    }
                }

                assert!(self.out_deque.is_empty());
            } else {
                trace!("   --> queueing chunk");

                self.out_deque.push(chunk);
                return;
            }
        }

        self.out_is_ready = false;
        self.out_body = None;
    }

    fn try_poll_in_body(&mut self) -> Poll<Option<T::BodyIn>, T::Error> {
        match self.in_body {
            Some(ref mut b) => b.poll(),
            None => {
                trace!(" !!! no in body??");
                Ok(Async::NotReady)
            }
        }
    }

    /// Write as many buffered body chunks to the sender
    fn flush_out_body(&mut self) -> io::Result<()> {
        {
            let sender = match self.out_body {
                Some(ref mut sender) => sender,
                None => {
                    assert!(self.out_deque.is_empty(), "pending out frames but no sender");
                    return Ok(());
                }
            };

            self.out_is_ready = true;

            loop {
                // Pop a pending frame
                let msg = match self.out_deque.pop() {
                    Some(Some(msg)) => msg,
                    Some(None) => break,
                    None => {
                        // No more frames to flush
                        return Ok(());
                    }
                };

                // Errors represent the last message to send
                let done = msg.is_err();

                match sender.start_send(msg) {
                    Ok(AsyncSink::Ready) => {}
                    Ok(AsyncSink::NotReady(msg)) => {
                        trace!("   --> not ready");

                        // Sender not ready
                        self.out_deque.push_front(Some(msg));
                        self.out_is_ready = false;

                        return Ok(());
                    }
                    Err(_) => {
                        // The receiving end dropped interest in the body
                        // stream. In this case, the sender and the frame
                        // buffer is dropped. If future body frames are
                        // received, the sender will be gone and the frames
                        // will be dropped.
                        //
                        // TODO: Notify transport
                        break;
                    }
                }

                if done {
                    break
                }
            }
        }

        // At this point, the outbound body is complete.
        self.out_deque.clear();
        self.out_is_ready = false;
        self.out_body = None;

        Ok(())
    }
}

impl<T> fmt::Debug for Exchange<T>
    where T: Dispatch + fmt::Debug,
          T::In: fmt::Debug,
          T::Out: fmt::Debug,
          T::BodyIn: fmt::Debug,
          T::BodyOut: fmt::Debug,
          T::Error: fmt::Debug,
          T::Stream: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.debug_struct("Exchange")
            .field("request", &self.request)
            .field("responded", &self.responded)
            .field("out_body", &"Sender { ... }")
            .field("out_deque", &"FrameDeque { ... }")
            .field("out_is_ready", &self.out_is_ready)
            .field("in_body", &self.in_body)
            .finish()
    }
}

impl<T: Dispatch> Request<T> {
    fn is_none(&self) -> bool {
        match *self {
            Request::In => true,
            Request::Out(None) => true,
            _ => false,
        }
    }
}

impl WriteState {
    fn transport_not_write_ready(&mut self) {
        if *self == WriteState::Wrote {
            *self = WriteState::Blocked;
        }
    }

    fn wrote_frame(&mut self) {
        if *self == WriteState::NoWrite {
            *self = WriteState::Wrote;
        }
    }
}

fn assert_send<T>(s: &mut T, item: T::SinkItem) -> Result<(), T::SinkError>
    where T: Sink
{
    match try!(s.start_send(item)) {
        AsyncSink::Ready => Ok(()),
        AsyncSink::NotReady(_) => {
            panic!("sink reported itself as ready after `poll_ready` but was \
                    then unable to accept a message")
        }
    }
}

/*
 *
 * ===== impl MultiplexMessage =====
 *
 */

impl<T, B, E> MultiplexMessage<T, B, E> {
    /// Create a new MultiplexMessage
    pub fn new(id: RequestId, message: Message<T, B>) -> MultiplexMessage<T, B, E> {
        MultiplexMessage {
            id: id,
            message: Ok(message),
            solo: false,
        }
    }

    /// Create a new errored MultiplexMessage
    pub fn error(id: RequestId, error: E) -> MultiplexMessage<T, B, E> {
        MultiplexMessage {
            id: id,
            message: Err(error),
            solo: false,
        }
    }
}

impl<T: Dispatch> Sink for DispatchSink<T> {
    type SinkItem = <T::Transport as Sink>::SinkItem;
    type SinkError = io::Error;

    fn start_send(&mut self, item: Self::SinkItem)
                  -> StartSend<Self::SinkItem, io::Error>
    {
        self.inner.transport().start_send(item)
    }

    fn poll_complete(&mut self) -> Poll<(), io::Error> {
        self.inner.transport().poll_complete()
    }

    fn close(&mut self) -> Poll<(), io::Error> {
        self.inner.transport().close()
    }
}
