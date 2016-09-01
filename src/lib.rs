//! A collection of components for rapid protocol development

#![deny(warnings, missing_docs)]

extern crate bytes;
extern crate futures;
extern crate slab;
extern crate take;
extern crate tokio_core;
extern crate tokio_service;

#[macro_use]
extern crate log;

pub mod io;
pub mod pipeline;
pub mod server;

mod service;

pub use self::service::{Service, NewService, SimpleService, simple_service};
