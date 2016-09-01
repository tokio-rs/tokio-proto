extern crate env_logger;
extern crate futures;
extern crate lazycell;
extern crate mio;
extern crate take;
extern crate tokio_proto;
extern crate tokio_core;

#[macro_use]
extern crate log;

mod support;

// Tests
mod test_io;
mod test_pipeline_client;
mod test_pipeline_server;
mod test_server;
