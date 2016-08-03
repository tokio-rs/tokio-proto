use {client, Service};
use super::{Frame, Transport, NewTransport};
use reactor::{ReactorHandle, Task, Tick};
use util::channel::{Receiver};
use util::future::{self, Complete, Val};
use mio::channel;
use std::io;
use std::net::SocketAddr;

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
    in_flight: Vec<Complete<T::Out, E>>,
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
            in_flight: Vec::with_capacity(16),
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

impl<T, E> Task for Client<T, E>
    where T: Transport,
          E: Send + 'static,
{
    fn tick(&mut self) -> io::Result<Tick> {
        trace!("pipeline::Client::tick");

        // The first action is always flushing the transport
        let mut flush = try!(self.transport.flush());

        // Process responses
        loop {
            match self.transport.read() {
                Ok(Some(frame)) => {
                    match frame {
                        Frame::Message(resp) => {
                            trace!("pipeline got response");

                            let c = self.in_flight.remove(0);
                            c.complete(resp);
                        }
                        Frame::Done => {
                            unimplemented!();
                        }
                        _ => unimplemented!(),
                    }
                }
                Ok(None) => break,
                Err(_) => unimplemented!(),
            }
        }

        // Process new requests
        while self.run && self.transport.is_writable() {
            match self.requests.recv() {
                Ok(Some((req, c))) => {
                    trace!("received request");

                    // Write the request to the transport
                    flush = try!(self.transport.write(Frame::Message(req)));
                    self.in_flight.push(c);
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

        if !self.run && flush.is_some() && self.in_flight.is_empty() {
            return Ok(Tick::Final);
        }

        // Tick again later
        Ok(Tick::WouldBlock)
    }
}
