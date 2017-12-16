extern crate bytes;
extern crate futures;
extern crate tokio_core;
extern crate tokio_io;
extern crate tokio_proto;
extern crate tokio_service;

use std::io;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use bytes::BytesMut;
use futures::Future;
use tokio_core::reactor::Core;
use tokio_io::{AsyncRead, AsyncWrite};
use tokio_io::codec::{Decoder, Encoder, Framed};
use tokio_proto::TcpClient;
use tokio_proto::pipeline::ClientProto;
use tokio_service::Service;

struct DummyCodec;

impl Decoder for DummyCodec {
    type Item = Vec<u8>;
    type Error = io::Error;

    fn decode(&mut self, buf: &mut BytesMut) -> io::Result<Option<Self::Item>> {
        if buf.len() > 0 {
            let len = buf.len();
            let bytes = buf.split_to(len);

            Ok(Some(bytes.iter().cloned().collect()))
        } else {
            Ok(None)
        }
    }
}

impl Encoder for DummyCodec {
    type Item = Vec<u8>;
    type Error = io::Error;

    fn encode(&mut self, msg: Vec<u8>, buf: &mut BytesMut) -> io::Result<()> {
        buf.extend(&msg);

        Ok(())
    }
}

struct DummyProtocol;

impl<T> ClientProto<T> for DummyProtocol
where
    T: AsyncRead + AsyncWrite + 'static,
{
    type Request = Vec<u8>;
    type Response = Vec<u8>;
    type Transport = Framed<T, DummyCodec>;
    type BindTransport = Result<Self::Transport, io::Error>;

    fn bind_transport(&self, io: T) -> Self::BindTransport {
        Ok(io.framed(DummyCodec))
    }
}

fn serve(address: SocketAddr) {
    let listener = TcpListener::bind(address).unwrap();
    let (mut connection, _address) = listener.accept().unwrap();
    let mut buffer = [0u8; 100];

    while let Ok(bytes) = connection.read(&mut buffer) {
        if bytes == 0 {
            break;
        }

        connection.write(&buffer[0..bytes]).unwrap();

        thread::sleep(Duration::from_millis(100));
    }
}

fn start_server(address: SocketAddr) -> Arc<Mutex<bool>> {
    let finished = Arc::new(Mutex::new(false));
    let finished_clone = finished.clone();

    thread::spawn(move || {
        serve(address);

        let mut has_finished = finished.lock().unwrap();

        *has_finished = true;
    });

    finished_clone
}

fn send_message(reactor: &mut Core, address: &SocketAddr) {
    // Give some time for server to start
    thread::sleep(Duration::from_millis(1000));

    let handle = reactor.handle();
    let client = TcpClient::new(DummyProtocol);
    let connect = client.connect(address, &handle);

    let send = connect.and_then(|connection| {
        connection.call("hello".as_bytes().iter().cloned().collect())
    });

    reactor.run(send).unwrap();
}

fn check_server_has_finished(finished: Arc<Mutex<bool>>) {
    let finished = finished.lock().unwrap();

    assert_eq!(*finished, true);
}

#[test]
fn reactor_is_dropped() {
    let address: SocketAddr = "127.0.0.1:55136".parse().unwrap();
    let finished = start_server(address.clone());

    {
        let mut reactor = Core::new().unwrap();

        send_message(&mut reactor, &address);
    }

    thread::sleep(Duration::from_millis(1000));

    check_server_has_finished(finished);
}

#[test]
fn reactor_is_not_dropped() {
    let address: SocketAddr = "127.0.0.1:55137".parse().unwrap();
    let finished = start_server(address.clone());

    let mut reactor = Core::new().unwrap();

    send_message(&mut reactor, &address);

    thread::sleep(Duration::from_millis(1000));

    check_server_has_finished(finished);
}
