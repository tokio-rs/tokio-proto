use {client, Service};
use super::{Frame, Transport, NewTransport};
use reactor::{ReactorHandle, Task, Tick};
use util::channel::{Receiver};
use util::future::{self, Complete, Val};
use mio::channel;
use std::io;
use std::net::SocketAddr;
use std::collections::VecDeque;

// TODO:
//
// - On drop, drain in_flight and complete w/ broken pipe error

/// Client `Service` for the pipeline protocol.
///
/// Initiated requests are sent to the client pipeline task running on the
/// Reactor where they are processed. The response is returned by completing
/// the future.
pub struct ClientHandle<T, U, E> {
    tx: channel::Sender<(T, Complete<U, E>)>,
}

struct Client<T, E>
    where T: Transport,
          E: Send + 'static,
{
    run: bool,
    transport: T,
    requests: Receiver<(T::In, Complete<T::Out, E>)>,
    is_flushed: bool,
    in_flight: VecDeque<Complete<T::Out, E>>,
}

/// Connect to the given `addr` and handle using the given Transport and protocol pipelining.
pub fn connect<T>(reactor: &ReactorHandle, addr: SocketAddr, new_transport: T)
        -> ClientHandle<T::In, T::Out, T::Error>
        where T: NewTransport,
{
    use take::Take;

    let (tx, rx) = channel::channel();

    client::connect(reactor, addr, Take::new(move |socket| {
        // Let Tokio watch all the sources for events
        let rx = try!(Receiver::watch(rx));

        // Create the transport
        let transport = try!(new_transport.new_transport(socket));

        Ok(Client {
            run: true,
            transport: transport,
            requests: rx,
            is_flushed: true,
            in_flight: VecDeque::with_capacity(32),
        })
    }));

    ClientHandle { tx: tx }
}

impl<T, U, E> Service for ClientHandle<T, U, E>
    where T: Send + 'static,
          U: Send + 'static,
          E: Send + 'static,
{
    type Req = T;
    type Resp = U;
    type Error = E;
    type Fut = Val<U, E>;

    fn call(&self, request: T) -> Val<U, E> {
        let (c, val) = future::pair();

        // TODO: handle error
        self.tx.send((request, c)).ok().unwrap();

        val
    }
}

impl<T, U, E> Clone for ClientHandle<T, U, E>
    where T: Send + 'static,
          U: Send + 'static,
          E: Send + 'static,
{
    fn clone(&self) -> ClientHandle<T, U, E> {
        ClientHandle { tx: self.tx.clone() }
    }
}

impl<T, E> Client<T, E>
    where T: Transport,
          E: Send + 'static,
{
    fn is_done(&self) -> bool {
        !self.run && self.is_flushed && self.in_flight.is_empty()
    }

    fn flush(&mut self) -> io::Result<()> {
        self.is_flushed = try!(self.transport.flush()).is_some();
        Ok(())
    }

    fn read_response_frames(&mut self) -> io::Result<()> {
        trace!("pipeline trying to read transport");

        while let Some(frame) = try!(self.transport.read()) {
            try!(self.process_response_frame(frame));
        }

        Ok(())
    }

    fn process_response_frame(&mut self, frame: Frame<T::Out, T::Error>)
        -> io::Result<()>
    {
        match frame {
            Frame::Message(response) => {
                trace!("pipeline got response");

                if let Some(complete) = self.in_flight.pop_front() {
                    complete.complete(response);
                } else {
                    return Err(io::Error::new(io::ErrorKind::Other, "request / response mismatch"));
                }
            }
            Frame::Done => {
                unimplemented!();
            }
            _ => unimplemented!(),
        }

        Ok(())
    }

    fn write_request_frames(&mut self) -> io::Result<()> {
        // Process new requests
        while self.run && self.transport.is_writable() {
            // Try to get a new request frame
            match self.requests.recv() {
                Ok(Some((request, complete))) => {
                    trace!("received request");

                    // Write the request to the transport
                    try!(self.transport.write(Frame::Message(request)));

                    // Track complete handle
                    self.in_flight.push_back(complete);
                }
                Ok(None) => {
                    trace!("request queue is empty");
                    break
                }
                Err(_) => {
                    // An error on receive can only happen when the other half
                    // disconnected. In this case, the client needs to be
                    // shutdown
                    self.run = false;
                    break;
                }
            }
        }

        Ok(())
    }
}

impl<T, E> Task for Client<T, E>
    where T: Transport,
          E: Send + 'static,
{
    fn tick(&mut self) -> io::Result<Tick> {
        trace!("pipeline::Client::tick");

        // Always flush the transport first
        try!(self.flush());

        // First, read data from the socket
        try!(self.read_response_frames());

        // Write all pendign request frames
        try!(self.write_request_frames());

        // Try flushing buffered writes
        try!(self.flush());

        if self.is_done() {
            return Ok(Tick::Final);
        }

        // Tick again later
        Ok(Tick::WouldBlock)
    }
}
