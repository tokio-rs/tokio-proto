use std::{fmt, io};
use std::sync::{Arc, Mutex};
use std::sync::mpsc::{self, Sender, Receiver};

use futures::{Future, Async};
use lazycell::LazyCell;
use mio::{self, Evented, Ready, PollOpt, Registration, SetReadiness, Token};
use tokio_proto::io::Readiness;
use tokio_core::io::IoFuture;
use tokio_core::{ReadinessStream, LoopHandle};

pub struct Transport<In, Out> {
    tx: Sender<Write<In>>,
    source: ReadinessStream<Io<Out>>,
    pending: Option<In>,
}

pub struct NewTransport<In, Out> {
    tx: Sender<Write<In>>,
    inner: Arc<Mutex<Inner<Out>>>,
    handle: LoopHandle,
}

pub struct TransportHandle<In, Out> {
    // A Receiver is needed in order to block waiting for messages
    rx: Receiver<Write<In>>,
    inner: Arc<Mutex<Inner<Out>>>,
}

// Used to register w/ mio
struct Io<Out> {
    registration: LazyCell<Registration>,
    inner: Arc<Mutex<Inner<Out>>>,
}

// Shared between `TransportHandle` and `Transport`
struct Inner<Out> {
    // Messages the transport can read
    read_buffer: Vec<io::Result<Out>>,
    // What the next write will do
    write_capability: Vec<WriteCap>,
    // Signals to the reactor that readiness changed
    set_readiness: LazyCell<SetReadiness>,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum WriteCap {
    Write,
    Flush,
}

#[derive(Debug, Clone, Copy)]
enum Write<T> {
    Write(T),
    Flush,
    Drop,
}

/// Create a new mock transport pair
pub fn transport<In, Out>(handle: LoopHandle) -> (TransportHandle<In, Out>, NewTransport<In, Out>) {
    let (tx, rx) = mpsc::channel();

    let inner = Arc::new(Mutex::new(Inner {
        read_buffer: vec![],
        write_capability: vec![],
        set_readiness: LazyCell::new(),
    }));

    let new_transport = NewTransport {
        handle: handle,
        tx: tx,
        inner: inner.clone(),
    };

    let handle = TransportHandle {
        rx: rx,
        inner: inner,
    };

    (handle, new_transport)
}

impl<In: fmt::Debug, Out> TransportHandle<In, Out> {

    /// Send a message to the transport.
    ///
    /// The transport will become readable and the next call to `::read()` will
    /// return the given message.
    pub fn send(&self, v: Out) {
        self.inner.lock().unwrap().send(Ok(v));
    }

    /// Send an error to the transport;
    ///
    /// The transport will become readable and the next call to `::read()` will
    /// return the given error
    pub fn error(&self, e: io::Error) {
        self.inner.lock().unwrap().send(Err(e));
    }

    /// Allow the transport to write a message.
    pub fn allow_write(&self) {
        self.inner.lock().unwrap().allow_write(WriteCap::Write);
    }

    /// Receive a write from the transport
    pub fn next_write(&self) -> In {
        match self.rx.recv().unwrap() {
            Write::Write(v) => v,
            Write::Flush => panic!("expected write; actual=Flush"),
            Write::Drop => panic!("expected write; actual=Drop"),
        }
    }

    /// Allow the transport to attempt to flush a message
    pub fn allow_flush(&self) {
        self.inner.lock().unwrap().allow_write(WriteCap::Flush);
    }

    pub fn assert_flush(&self) {
        match self.rx.recv().unwrap() {
            Write::Flush => {},
            Write::Write(v) => panic!("expected flush; actual={:?}", v),
            Write::Drop => panic!("expected flush; actual=Drop"),
        }
    }

    pub fn allow_and_assert_flush(&self) {
        self.allow_flush();
        self.assert_flush();
    }

    pub fn assert_drop(&self) {
        match self.rx.recv().unwrap() {
            Write::Drop => {},
            Write::Write(v) => panic!("expected flush; actual={:?}", v),
            Write::Flush => panic!("expected write; actual=Flush"),
        }
    }

    pub fn allow_and_assert_drop(&self) {
        self.allow_write();
        self.assert_drop();
    }

    pub fn assert_no_write(&self, ms: u64) {
        // Unfortunately, mpsc::channel() does not support timeouts on recv, so
        // for now just sleep
        super::sleep_ms(ms);

        if let Ok(v) = self.rx.try_recv() {
            panic!("expected no write; received={:?}", v);
        }
    }
}

impl<In, Out> ::tokio_proto::io::Transport for Transport<In, Out>
    where In: fmt::Debug,
{
    type In = In;
    type Out = Out;

    /// Read a message frame from the `Transport`
    fn read(&mut self) -> io::Result<Option<Out>> {
        match self.source.get_ref().inner.lock().unwrap().recv() {
            Some(Ok(v)) => Ok(Some(v)),
            Some(Err(e)) => Err(e),
            None => {
                self.source.need_read();
                Ok(None)
            }
        }
    }

    /// Write a message frame to the `Transport`
    fn write(&mut self, req: In) -> io::Result<Option<()>> {
        if !self.is_writable() {
            panic!("cannot write request when not writable");
        }

        trace!("transport write; frame={:?}", req);

        self.pending = Some(req);
        self.flush()
    }

    fn flush(&mut self) -> io::Result<Option<()>> {
        if !try!(self.source.poll_write()).is_ready() {
            return Ok(None)
        }

        if self.pending.is_none() {
            return Ok(Some(()));
        }

        trace!("transport flush");

        let mut inner = self.source.get_ref().inner.lock().unwrap();

        while let Some(cap) = shift(&mut inner.write_capability) {
            inner.set_readiness();

            match cap {
                WriteCap::Flush => self.tx.send(Write::Flush).unwrap(),
                WriteCap::Write => {
                    let val = self.pending.take().unwrap();
                    self.tx.send(Write::Write(val)).unwrap();
                    return Ok(Some(()));
                }
            }
        }

        self.source.need_write();
        Ok(None)
    }
}

impl<In, Out> Drop for Transport<In, Out> {
    fn drop(&mut self) {
        trace!("transport dropping");
        let _ = self.tx.send(Write::Drop);
    }
}

fn shift<T>(queue: &mut Vec<T>) -> Option<T> {
    if queue.len() == 0 {
        return None;
    }

    Some(queue.remove(0))
}

impl<In, Out> Readiness for Transport<In, Out> {

    fn is_readable(&self) -> bool {
        match self.source.poll_read() {
            Ok(Async::Ready(())) => true,
            _ => false,
        }
    }

    fn is_writable(&self) -> bool {
        match self.source.poll_write() {
            Ok(Async::Ready(())) => true,
            _ => false,
        }
    }
}

impl<In, Out> NewTransport<In, Out>
    where In: Send + 'static,
          Out: Send + 'static,
{
    pub fn new_transport(self) -> IoFuture<Transport<In, Out>> {
        let NewTransport { tx, inner, handle } = self;

        let io = Io {
            registration: LazyCell::new(),
            inner: inner,
        };

        let source = ReadinessStream::new(handle, io);

        source.map(|source| {
            Transport {
                tx: tx,
                source: source,
                pending: None,
            }
        }).boxed()
    }
}

impl<Out> Inner<Out> {
    fn send(&mut self, v: io::Result<Out>) {
        self.read_buffer.push(v);
        self.set_readiness();
    }

    fn recv(&mut self) -> Option<io::Result<Out>> {
        let ret = shift(&mut self.read_buffer);
        self.set_readiness();
        ret
    }

    fn allow_write(&mut self, cap: WriteCap) {
        trace!("allowing write; kind={:?}", cap);
        self.write_capability.push(cap);
        self.set_readiness();
    }

    fn set_readiness(&self) {
        if let Some(h) = self.set_readiness.borrow() {
            let mut readiness = Ready::none();

            if self.read_buffer.len() > 0 {
                readiness = readiness | Ready::readable();
            }

            if self.write_capability.len() > 0 {
                readiness = readiness | Ready::writable();
            }

            let orig = h.readiness();

            if readiness != orig {
                trace!("updating readiness; after={:?}; before={:?}", readiness, orig);
            }

            h.set_readiness(readiness).unwrap();
        }
    }
}


impl<T> Evented for Io<T> {
    fn register(&self, poll: &mio::Poll, token: Token, interest: Ready, opts: PollOpt) -> io::Result<()> {
        if self.registration.filled() {
            return Err(io::Error::new(io::ErrorKind::Other, "already registered"));
        }

        let (registration, set_readiness) = Registration::new(poll, token, interest, opts);
        let inner = self.inner.lock().unwrap();

        self.registration.fill(registration);
        inner.set_readiness.fill(set_readiness);

        inner.set_readiness();

        Ok(())
    }

    fn reregister(&self, poll: &mio::Poll, token: Token, interest: Ready, opts: PollOpt) -> io::Result<()> {
        match self.registration.borrow() {
            Some(registration) => registration.update(poll, token, interest, opts),
            None => Err(io::Error::new(io::ErrorKind::Other, "receiver not registered")),
        }
    }

    fn deregister(&self, poll: &mio::Poll) -> io::Result<()> {
        match self.registration.borrow() {
            Some(registration) => registration.deregister(poll),
            None => Err(io::Error::new(io::ErrorKind::Other, "receiver not registered")),
        }
    }
}
