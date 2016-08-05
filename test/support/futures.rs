use futures::Future;
use std::sync::mpsc;

pub fn await<F: Future>(f: F) -> Result<F::Item, F::Error> {
    let (tx, rx) = mpsc::channel();

    f.then(move |res| {
        tx.send(res).unwrap();
        Ok::<(), ()>(())
    }).forget();

    rx.recv().unwrap()
}
