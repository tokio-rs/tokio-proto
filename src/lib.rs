//! A collection of components for rapid protocol development

#![deny(warnings, missing_docs)]

extern crate bytes;
extern crate futures;
extern crate slab;
extern crate take;
extern crate rand;
extern crate smallvec;
extern crate tokio_core;
extern crate tokio_service;

#[macro_use]
extern crate log;

pub mod multiplex;
pub mod pipeline;
pub mod server;

mod framing;
mod io;
mod service;

pub use framing::{Framed, Parse, Serialize};
pub use io::{TryRead, TryWrite};
pub use service::{NewService};
