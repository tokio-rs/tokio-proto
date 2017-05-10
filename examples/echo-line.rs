
extern crate tokio_proto;
extern crate tokio_core;
extern crate tokio_io;
extern crate futures;
extern crate tokio_service;
extern crate bytes;

use std::io;

use futures::future;
use futures::{Future, BoxFuture};
use tokio_io::{AsyncRead, AsyncWrite};
use tokio_io::codec::{Framed, Encoder, Decoder};
use bytes::BytesMut;
use tokio_proto::TcpServer;
use tokio_proto::pipeline::ServerProto;
use tokio_service::Service;

struct LineCodec;

impl Encoder for LineCodec {
  type Item = String;
  type Error = io::Error;

  fn encode(&mut self, msg: String, buf: &mut BytesMut) -> io::Result<()> {
      buf.extend(msg.as_bytes());
      buf.extend(b"\n");
      Ok(())
  }
}

impl Decoder for LineCodec {
  type Item = String;
  type Error = io::Error;

  fn decode(&mut self, buf: &mut BytesMut) -> io::Result<Option<String>> {
      if let Some(i) = buf.iter().position(|&b| b == b'\n') {
          // remove the serialized frame from the buffer.
          let line = buf.split_to(i);

          // Also remove the '\n'
          buf.split_to(1);

          // Turn this data into a UTF string and return it in a Frame.
          match std::str::from_utf8(&line) {
              Ok(s) => Ok(Some(s.to_string())),
              Err(_) => Err(io::Error::new(io::ErrorKind::Other,
                                           "invalid UTF-8")),
          }
      } else {
          Ok(None)
      }
  }
}

struct LineProto;

impl<T: AsyncRead + AsyncWrite + 'static> ServerProto<T> for LineProto {
    type Request = String;
    type Response = String;
    type Transport = Framed<T, LineCodec>;
    type BindTransport = Result<Self::Transport, io::Error>;

    fn bind_transport(&self, io: T) -> Self::BindTransport {
        Ok(io.framed(LineCodec))
    }
}

struct Echo;

impl Service for Echo {
    type Request = String;
    type Response = String;
    type Error = io::Error;
    type Future = BoxFuture<Self::Response, Self::Error>;

    fn call(&self, req: Self::Request) -> Self::Future {
        future::ok(req).boxed()
    }
}

fn main() {
    // Specify the localhost address
    let addr = "0.0.0.0:12345".parse().unwrap();

    // The builder requires a protocol and an address
    let server = TcpServer::new(LineProto, addr);

    // We provide a way to *instantiate* the service for each new
    // connection; here, we just immediately return a new instance.
    server.serve(|| Ok(Echo));
}
