extern crate futures;
extern crate tokio_core;
extern crate tokio_proto;
extern crate tokio_service;

use std::io;
use futures::{Future, BoxFuture};
use tokio_core::io::{Io, Codec, Framed, EasyBuf};
use tokio_proto::TcpServer;
use tokio_proto::streaming::{Message, Body};
use tokio_proto::streaming::pipeline;
use tokio_proto::streaming::multiplex;
use tokio_service::Service;



#[derive(Default)]
struct PipelineCodec;

impl Codec for PipelineCodec {
    type In = pipeline::Frame<u32, (), io::Error>;
    type Out = pipeline::Frame<u32, u32, io::Error>;

    fn decode(&mut self, buf: &mut EasyBuf) -> Result<Option<Self::In>, io::Error> {
        Ok(None)
    }

    fn encode(&mut self, item: Self::Out, buf: &mut Vec<u8>) -> io::Result<()> {
        Ok(())
    }
}

#[derive(Default)]
struct MultiplexCodec;

impl Codec for MultiplexCodec {
    type In = multiplex::Frame<u32, (), io::Error>;
    type Out = multiplex::Frame<u32, u32, io::Error>;

    fn decode(&mut self, buf: &mut EasyBuf) -> Result<Option<Self::In>, io::Error> {
        Ok(None)
    }

    fn encode(&mut self, item: Self::Out, buf: &mut Vec<u8>) -> io::Result<()> {
        Ok(())
    }
}

struct PipelineProto;

impl<T: Io + 'static> pipeline::ServerProto<T> for PipelineProto {
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

impl<T: Io + 'static> multiplex::ServerProto<T> for MultiplexProto {
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

    fn call(&self, req: Self::Request) -> Self::Future {
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
