# tokio-proto

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

## Getting Help
If you have questions or need further help getting started, consider joining
the chat in our [Gitter Channel](http://gitter.im/tokio-rs/tokio).

## Built with Tokio

**[Tokio Line](https://github.com/tokio-rs/tokio-line)**

An example of how to implement a client and server with Tokio. The
protocol consists of UTF-8 strings where messages are `\n` terminated.

**[Tokio Hyper](https://github.com/tokio-rs/tokio-hyper)**

A Tokio HTTP server built on top of Hyper. Full Tokio integration is
coming to Hyper in version 0.10.

**[Tokio Redis](https://github.com/tokio-rs/tokio-redis)**

A basic Redis client built with Tokio.

## Related Articles

If you're interested in building with Tokio you may want to check out the following articles:

* [Introduction to Futures](http://aturon.github.io/blog/2016/08/11/futures/) which this crate is based on.
* [Carl's Introduction to Tokio](https://medium.com/@carllerche/announcing-tokio-df6bb4ddb34#.s2jti29o3) to the crate.

## License

Tokio is primarily distributed under the terms of both the MIT license
and the Apache License (Version 2.0), with portions covered by various
BSD-like licenses.

See LICENSE-APACHE, and LICENSE-MIT for details.
