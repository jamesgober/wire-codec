<h1 align="center">
    <img width="99" alt="Rust logo" src="https://raw.githubusercontent.com/jamesgober/rust-collection/72baabd71f00e14aa9184efcb16fa3deddda3a0a/assets/rust-logo.svg">
    <br>
    <strong>wire-codec</strong>
    <br>
    <sup><sub>BINARY FRAMING AND ENCODING PRIMITIVES</sub></sup>
</h1>

<p align="center">
    <a href="https://crates.io/crates/wire-codec"><img alt="crates.io" src="https://img.shields.io/crates/v/wire-codec.svg"></a>
    <a href="https://docs.rs/wire-codec"><img alt="docs.rs" src="https://docs.rs/wire-codec/badge.svg"></a>
    <a href="https://github.com/rust-lang/rfcs/blob/master/text/2495-min-rust-version.md" title="MSRV"><img alt="MSRV" src="https://img.shields.io/badge/MSRV-1.75%2B-blue"></a>
    <a href="https://github.com/jamesgober/wire-codec/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/jamesgober/wire-codec/actions/workflows/ci.yml/badge.svg"></a>
</p>

<p align="center">Length-prefixed, delimiter-based, and custom framing strategies. Built-in varint, zigzag, bitfield, and packed-struct encoding. Runtime-agnostic.</p>

## What it does

Binary frame codec and protocol codec toolkit. Length-prefixed, delimiter-based, and custom framing strategies. Built-in varint, zigzag, bitfield, and packed struct encoding. Runtime-agnostic foundation under network-protocol crates.

---

## Quick start

```toml
[dependencies]
wire-codec = "0.2"
```

```rust
use wire_codec::WriteBuf;
use wire_codec::framing::{Endian, Framer, LengthPrefixed, LengthWidth};

let framer = LengthPrefixed::new(LengthWidth::U16, Endian::Big);

let mut out = [0u8; 32];
let mut buf = WriteBuf::new(&mut out);
framer.write_frame(b"ping", &mut buf).unwrap();
let n = buf.position();

let frame = framer.next_frame(&out[..n]).unwrap().unwrap();
assert_eq!(frame.payload(), b"ping");
```

See [`docs/API.md`](docs/API.md) for the full API reference.

---

## Standards

- **REPS** governs every decision. See [REPS.md](REPS.md).
- **MSRV:** Rust 1.75.
- **Edition:** 2024.
- **Cross-platform:** Linux, macOS, Windows.

---

## License

Dual-licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT License ([LICENSE-MIT](LICENSE-MIT))

at your option.


<!-- FOOT COPYRIGHT
################################################# -->
<div align="center">
  <h2></h2>
  <sup>COPYRIGHT <small>&copy;</small> 2026 <strong>JAMES GOBER.</strong></sup>
</div>