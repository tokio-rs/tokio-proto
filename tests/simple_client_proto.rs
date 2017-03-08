extern crate futures;
extern crate tokio_core;
extern crate tokio_proto;
extern crate tokio_service;
extern crate tokio_io;
extern crate bytes;

use std::str;
use std::io::{self, ErrorKind};

use bytes::{BytesMut, BufMut};
use futures::{Future};
use tokio_io::{AsyncRead, AsyncWrite};
use tokio_io::codec::{Encoder, Decoder, Framed};
use tokio_core::reactor::Core;
use tokio_proto::pipeline::ClientProto;
use tokio_proto::TcpClient;

// First, we implement a *codec*, which provides a way of encoding and
// decoding messages for the protocol. See the documentation for `Codec` in
// `tokio-core` for more details on how that works.

#[derive(Default)]
pub struct IntCodec;

fn parse_u64(from: &[u8]) -> Result<u64, io::Error> {
    Ok(str::from_utf8(from)
       .map_err(|e| io::Error::new(ErrorKind::InvalidData, e))?
       .parse()
       .map_err(|e| io::Error::new(ErrorKind::InvalidData, e))?)
}

impl Decoder for IntCodec {
    type Item = u64;
    type Error = io::Error;

    // Attempt to decode a message from the given buffer if a complete
    // message is available; returns `Ok(None)` if the buffer does not yet
    // hold a complete message.
    fn decode(&mut self, buf: &mut BytesMut) -> Result<Option<u64>, io::Error> {
        if let Some(i) = buf.iter().position(|&b| b == b'\n') {
            // remove the line, including the '\n', from the buffer
            let full_line = buf.split_to(i + 1);

            // strip the'`\n'
            let slice = &full_line[..i];

            Ok(Some(parse_u64(slice)?))
        } else {
            Ok(None)
        }
    }

    // Attempt to decode a message assuming that the given buffer contains
    // *all* remaining input data.
    fn decode_eof(&mut self, buf: &mut BytesMut) -> io::Result<Option<u64>> {
        if buf.len() == 0 {
            Ok(None)
        } else {
            let amt = buf.len();
            Ok(Some(parse_u64(&buf.split_to(amt))?))
        }
    }
}



impl Encoder for IntCodec {
    type Item = u64;
    type Error = io::Error;

    fn encode(&mut self, item: u64, into: &mut BytesMut) -> io::Result<()> {
        into.put(item.to_string().as_bytes());
        Ok(())
    }
}

// Next, we implement the server protocol, which just hooks up the codec above.

pub struct IntProto;

impl<T: AsyncRead + AsyncWrite + 'static> ClientProto<T> for IntProto {
    type Request = u64;
    type Response = u64;
    type Transport = Framed<T, IntCodec>;
    type BindTransport = Result<Self::Transport, io::Error>;

    fn bind_transport(&self, io: T) -> Self::BindTransport {
        Ok(io.framed(IntCodec))
    }
}

fn is_clone<T: Clone>(_: &T) {
}

#[test]
fn test_clone() {
    // Don't want the code to run, only compile
    if false {
        let core = Core::new().unwrap();
        let builder = TcpClient::new(IntProto);
        let service = builder.connect(&"127.0.0.1:12345".parse().unwrap(), &core.handle()).wait().unwrap();
        is_clone(&service);
    }
}
