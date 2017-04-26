# tokio-proto

`tokio-proto` makes it easy to implement clients and servers for **request /
response** oriented protocols. It takes a [transport] and provides the request /
response API. It is a part of the [Tokio] platform.

[![Build Status](https://travis-ci.org/tokio-rs/tokio-proto.svg?branch=master)](https://travis-ci.org/tokio-rs/tokio-proto)

[Documentation](https://docs.rs/tokio-proto) |
[Gitter](https://gitter.im/tokio-rs/tokio) |
[Tutorial](https://tokio.rs)

[transport]: https://tokio.rs/docs/going-deeper-tokio/transports/
[Tokio]: https://tokio.rs

## Usage

First, add this to your `Cargo.toml`:

```toml
[dependencies]
tokio-proto = { git = "https://github.com/tokio-rs/tokio-proto" }
```

Next, add this to your crate:

```rust
extern crate tokio_proto;
```

You can find extensive examples and tutorials at
[https://tokio.rs](https://tokio.rs).

## Getting Help

If you have questions or need further help getting started, consider joining
the chat in our [Gitter Channel](http://gitter.im/tokio-rs/tokio).

## License

Tokio is primarily distributed under the terms of both the MIT license
and the Apache License (Version 2.0), with portions covered by various
BSD-like licenses.

See LICENSE-APACHE, and LICENSE-MIT for details.
