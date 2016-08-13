use {Service};
use super::{Error, Frame, Transport};
use reactor::{Task, Tick};
use util::future::AwaitQueue;
use std::io;

// TODO:
//
// - Wait for service readiness

/// A server `Task` that dispatches `Transport` messages to a `Service` using
/// protocol pipelining.
pub struct Server<S, T>
    where S: Service,
{
    run: bool,
    service: S,
    transport: T,
    is_flushed: bool,
    in_flight: AwaitQueue<S::Fut>,
}


impl<S, T, E> Server<S, T>
    where S: Service<Error = E>,
          T: Transport<In = S::Resp, Out = S::Req, Error = E>,
          E: From<Error<E>> + Send + 'static,
{
    /// Create a new pipeline `Server` dispatcher with the given service and
    /// transport
    pub fn new(service: S, transport: T) -> io::Result<Server<S, T>> {
        Ok(Server {
            run: true,
            service: service,
            transport: transport,
            is_flushed: true,
            in_flight: try!(AwaitQueue::with_capacity(32)),
        })
    }

    /// Returns true if the pipeline server dispatch has nothing left to do
    fn is_done(&self) -> bool {
        !self.run && self.is_flushed && self.in_flight.is_empty()
    }

    fn read_request_frames(&mut self) -> io::Result<()> {
        while self.run {
            trace!("pipeline trying to read transport");

            if let Some(frame) = try!(self.transport.read()) {
                try!(self.process_request_frame(frame));
            } else {
                break;
            }
        }

        Ok(())
    }

    fn process_request_frame(&mut self, frame: Frame<S::Req, E>) -> io::Result<()> {
        match frame {
            Frame::Message(request) => {
                trace!("pipeline got request");
                let response = self.service.call(request);

                // Fast path for immediate futures. This is to make
                // the micro benchmarks happy given the overhead
                // associated with using futures-rs
                if self.transport.is_writable() {
                    // Try to get the next completed future
                    if let Some(response) = self.in_flight.push_poll(response) {
                        try!(self.write_response(response));
                    }
                } else {
                    self.in_flight.push(response);
                }
            }
            Frame::Done => {
                trace!("received Frame::Done");
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

    fn write_response_frames(&mut self) -> io::Result<()> {
        while self.transport.is_writable() {
            trace!("pipeline transport is writable");

            // Try to get the next completed future
            match self.in_flight.poll() {
                Some(Ok(val)) => {
                    trace!("got in_flight value");
                    try!(self.transport.write(Frame::Message(val)));
                }
                Some(Err(e)) => {
                    trace!("got in_flight error");
                    try!(self.transport.write(Frame::Error(e)));
                },
                None => {
                    trace!("no response ready for write");
                    break;
                }
            }
        }

        Ok(())
    }

    fn write_response(&mut self, response: Result<S::Resp, S::Error>) -> io::Result<()> {
        match response {
            Ok(val) => {
                trace!("got in_flight value");
                try!(self.transport.write(Frame::Message(val)));
            }
            Err(e) => {
                trace!("got in_flight error");
                try!(self.transport.write(Frame::Error(e)));
            }
        }

        Ok(())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.is_flushed = try!(self.transport.flush()).is_some();
        Ok(())
    }
}

impl<S, T, E> Task for Server<S, T>
    where S: Service<Error = E>,
          T: Transport<In=S::Resp, Out=S::Req, Error = E>,
          E: From<Error<E>> + Send + 'static,
{
    fn tick(&mut self) -> io::Result<Tick> {
        trace!("pipeline::Server::tick");

        // Always flush the transport first
        try!(self.flush());

        // First read off data from the socket
        try!(self.read_request_frames());

        // Handle completed responses
        try!(self.write_response_frames());

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
