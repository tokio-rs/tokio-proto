//! Utility for managing a `futures::stream::Sender` and backpressure.
//!
//! Remove this if alexcrichton/futures-rs#issues/164 lands.
use futures::{Future, Async, Poll};
use futures::stream;

pub struct Sender<T, E> {
    inner: Option<State<T, E>>,
}

enum State<T, E> {
    Ready(stream::Sender<T, E>),
    Busy(stream::FutureSender<T, E>),
}

impl<T, E> Sender<T, E> {
    pub fn send(&mut self, t: Result<T, E>) {
        trace!("Sender::send");

        match self.inner.take() {
            Some(State::Ready(sender)) => {
                let busy = sender.send(t);
                self.inner = Some(State::Busy(busy));

                // Poll the sender to actually fire the message
                let _ = self.poll_ready();
            }
            _ => panic!("invalid internal state"),
        }
    }

    pub fn poll_ready(&mut self) -> Poll<(), ()> {
        trace!("Sender::poll_ready");

        match self.inner {
            Some(State::Ready(..)) => {
                trace!("   --> sender already ready");
                Ok(Async::Ready(()))
            }
            _ => {
                match self.inner.take() {
                    Some(State::Busy(mut sender)) => {
                        trace!("   --> polling future sender");

                        match sender.poll() {
                            Ok(Async::Ready(sender)) => {
                                trace!("   --> ready");
                                // The sender is ready
                                self.inner = Some(State::Ready(sender));
                                Ok(Async::Ready(()))
                            }
                            Ok(Async::NotReady) => {
                                self.inner = Some(State::Busy(sender));
                                Ok(Async::NotReady)
                            }
                            Err(_) =>{
                                Err(())
                            }
                        }
                    }
                    None => Err(()),
                    _ => panic!("invalid state"),
                }
            }
        }
    }
}

impl<T, E> From<stream::Sender<T, E>> for Sender<T, E> {
    fn from(src: stream::Sender<T, E>) -> Sender<T, E> {
        Sender { inner: Some(State::Ready(src)) }
    }
}
