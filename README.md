# Tokio

Tokio is a network application framework for rapid development and
highly scalable production deployments of clients and servers.

[API documentation](http://rust-doc.s3-website-us-east-1.amazonaws.com/tokio/master/tokio)

## Usage

First, add this to your `Cargo.toml`:

```toml
[dependencies]
tokio = { git = "https://github.com/tokio-rs/tokio" }
```

Next, add this to your crate:

```rust
extern crate tokio;
```

And then, use Tokio!

## Built with Tokio

**[Tokio Line](https://github.com/tokio-rs/tokio-line)**

An example of how to implement a client and server with Tokio. The
protocol consists of UTF-8 strings where messages are `\n` terminated.

**[Tokio Hyper](https://github.com/tokio-rs/tokio-hyper)**

A Tokio HTTP server built on top of Hyper. Full Tokio integration is
coming to Hyper in version 0.10.

**[Tokio Redis](https://github.com/tokio-rs/tokio-redis)**

A basic Redis client built with Tokio.

## License

Tokio is primarily distributed under the terms of both the MIT license
and the Apache License (Version 2.0), with portions covered by various
BSD-like licenses.

See LICENSE-APACHE, and LICENSE-MIT for details.
