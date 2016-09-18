//! A collection of components for rapid protocol development

#![deny(warnings, missing_docs)]

extern crate bytes;
extern crate slab;
extern crate take;
extern crate rand;
extern crate smallvec;
extern crate tokio_core;
extern crate tokio_service;

#[macro_use]
extern crate futures;

#[macro_use]
extern crate log;

pub mod multiplex;
pub mod pipeline;
pub mod server;

mod error;
mod framing;
mod io;

pub use error::Error;
pub use framing::{Framed, Parse, Serialize};
pub use io::{TryRead, TryWrite};
