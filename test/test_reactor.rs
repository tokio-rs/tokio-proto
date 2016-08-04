use tokio::io::Ready;
use tokio::reactor::{self, Config, Reactor, Task, Tick};
use std::io;
use std::sync::mpsc::{self, Sender};

#[test]
fn test_internal_source_state_is_cleaned_up() {
    use mio::{Evented, EventSet, Poll, PollOpt, Token};

    struct Foo;

    impl Evented for Foo {
        fn register(&self, _: &Poll, _: Token, _: EventSet, _: PollOpt) -> io::Result<()> {
            Ok(())
        }

        fn reregister(&self, _: &Poll, _: Token, _: EventSet, _: PollOpt) -> io::Result<()> {
            Ok(())
        }

        fn deregister(&self, _: &Poll) -> io::Result<()> {
            Ok(())
        }
    }

    let config = Config::new()
        .max_sources(1);

    // Create a reactor that will only accept a single source
    let reactor = Reactor::new(config).unwrap();

    reactor.handle().oneshot(|| {
        let foo = Foo;

        // Run this a few times, because even if we request a slab of size 1,
        // there could be greater capacity (usually 2 given rounding to the
        // nearest power of 2)
        for _ in 0..10 {
            let source = reactor::register_source(&foo, Ready::readable());
            assert!(source.is_ok());
        }

        reactor::shutdown();
    });

    assert!(reactor.run().is_ok());
}

#[test]
fn test_returning_error_from_task_terminates() {
    struct MyTask {
        tx: Sender<()>,
    }

    impl Task for MyTask {
        fn tick(&mut self) -> io::Result<Tick> {
            Err(io::Error::new(io::ErrorKind::Other, "boom"))
        }
    }

    impl Drop for MyTask {
        fn drop(&mut self) {
            let _ = self.tx.send(());
        }
    }

    let reactor = Reactor::default().unwrap();
    let handle = reactor.handle();
    reactor.spawn();

    let (tx, rx) = mpsc::channel();

    handle.schedule(MyTask { tx: tx });

    // Receive the cleanup notice
    rx.recv().unwrap();

    handle.shutdown();
}
