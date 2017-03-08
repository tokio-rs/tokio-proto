extern crate futures;
extern crate tokio_core;
extern crate tokio_proto;
extern crate tokio_service;
extern crate tokio_io;
extern crate bytes;

use std::io;

use bytes::BytesMut;
use futures::BoxFuture;
use tokio_io::{AsyncRead, AsyncWrite};
use tokio_io::codec::{Encoder, Decoder, Framed};
use tokio_proto::TcpServer;
use tokio_proto::streaming::{Message, Body};
use tokio_proto::streaming::pipeline;
use tokio_proto::streaming::multiplex;
use tokio_service::Service;

#[derive(Default)]
struct PipelineCodec;

impl Decoder for PipelineCodec {
    type Item = pipeline::Frame<u32, (), io::Error>;
    type Error = io::Error;

    fn decode(&mut self, _: &mut BytesMut) -> io::Result<Option<Self::Item>> {
        Ok(None)
    }
}

impl Encoder for PipelineCodec {
    type Item = pipeline::Frame<u32, u32, io::Error>;
    type Error = io::Error;

    fn encode(&mut self, _: Self::Item, _: &mut BytesMut) -> io::Result<()> {
        Ok(())
    }
}

#[derive(Default)]
struct MultiplexCodec;

impl Decoder for MultiplexCodec {
    type Item = multiplex::Frame<u32, (), io::Error>;
    type Error = io::Error;

    fn decode(&mut self, _: &mut BytesMut) -> io::Result<Option<Self::Item>> {
        Ok(None)
    }
}

impl Encoder for MultiplexCodec {
    type Item = multiplex::Frame<u32, u32, io::Error>;
    type Error = io::Error;

    fn encode(&mut self, _: Self::Item, _: &mut BytesMut) -> io::Result<()> {
        Ok(())
    }
}

struct PipelineProto;

impl<T: AsyncRead + AsyncWrite + 'static> pipeline::ServerProto<T> for PipelineProto {
    type Request = u32;
    type RequestBody = ();
    type Response = u32;
    type Error = io::Error;
    type ResponseBody = u32;
    type Transport = Framed<T, PipelineCodec>;
    type BindTransport = Result<Self::Transport, io::Error>;

    fn bind_transport(&self, io: T) -> Self::BindTransport {
        Ok(io.framed(PipelineCodec))
    }
}

struct MultiplexProto;

impl<T: AsyncRead + AsyncWrite + 'static> multiplex::ServerProto<T> for MultiplexProto {
    type Request = u32;
    type RequestBody = ();
    type Response = u32;
    type Error = io::Error;
    type ResponseBody = u32;
    type Transport = Framed<T, MultiplexCodec>;
    type BindTransport = Result<Self::Transport, io::Error>;

    fn bind_transport(&self, io: T) -> Self::BindTransport {
        Ok(io.framed(MultiplexCodec))
    }
}



struct TestService;
impl Service for TestService {
    type Request = Message<u32, Body<(), io::Error>>;
    type Response = Message<u32, Body<u32, io::Error>>;
    type Error = io::Error;
    type Future = BoxFuture<Self::Response, io::Error>;

    fn call(&self, _: Self::Request) -> Self::Future {
        unimplemented!();
    }
}

#[test]
fn test_streaming_pipeline_framed() {
    // Test we can use Framed from tokio-core for (simple) streaming pipeline protocols
    // Don't want this to run, only compile
    if false {
        let addr = "0.0.0.0:12345".parse().unwrap();
        TcpServer::new(PipelineProto, addr)
            .serve(|| Ok(TestService));
    }
}

#[test]
fn test_streaming_multiplex_framed() {
    // Test we can use Framed from tokio-core for (simple) streaming multiplex protocols
    // Don't want this to run, only compile
    if false {
        let addr = "0.0.0.0:12345".parse().unwrap();
        TcpServer::new(MultiplexProto, addr)
            .serve(|| Ok(TestService));
    }
}
