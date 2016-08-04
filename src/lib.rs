//! Tokio is a network application framework for rapid development and highly
//! scalable production deployments of clients and servers.
//!
//! Tokio consists of multiple layers.
//!
//! # Service
//!
//! At a high level, Tokio provides a `Service` trait which provides a unified
//! API for writing clients and servers as well as the ability to build
//! reusable middleware components.
//!
//! The service trait is decoupled from any notion of a runtime.
//!
//! # Reactor
//!
//! The Tokio Reactor is a lightweight, event driven, task scheduler. It
//! accepts tasks, which are small units of work and schedules them for
//! execution when their dependent I/O sources (TCP sockets, timers, etc...)
//! are ready.
//!
//! The reactor and task exist in the `reactor` module.
//!
//! # Protocol building blocks
//!
//! Tokio aims to provide all the pieces necessary for rapidly developing
//! protocol implementations. These components exist in the `proto` module.

#![deny(warnings, missing_docs)]

extern crate mio;
extern crate slab;
extern crate futures;
extern crate take;

#[macro_use]
extern crate scoped_tls;

#[macro_use]
extern crate log;

pub mod client;
pub mod io;
pub mod proto;
pub mod reactor;
pub mod server;
pub mod tcp;
pub mod util;

mod service;

pub use self::service::{Service, NewService, SimpleService, simple_service};
