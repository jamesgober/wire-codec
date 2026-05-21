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
    <a href="https://github.com/jamesgober/wire-codec/blob/main/LICENSE-APACHE"><img alt="License" src="https://img.shields.io/badge/license-Apache--2.0%20OR%20MIT-blue"></a>
</p>

<p align="center"><b>A runtime-agnostic toolkit for binary framing and codec composition.</b></p>

<p align="center">
Zero-copy buffer cursors, LEB128 varint and zigzag integer codecs, MSB-first
bitfield cursors, and length-prefixed and delimited frame extraction — all
allocation-free, all <code>no_std</code>-friendly, all behind a small,
stable API.
</p>

---

## Why wire-codec?

Most binary protocols boil down to three jobs: cut a byte stream into frames,
turn integers into compact wire representations, and pack tagged fields into
records. The Rust ecosystem has good crates for each of those jobs in
isolation, but they tend to assume a runtime (`tokio`, `async-std`), or a
specific serialization model (`serde`, `prost`), or a buffer abstraction
(`bytes::Bytes`, `Vec<u8>`) that propagates outward through your code.

`wire-codec` does not. It works on borrowed byte slices: you bring the buffer,
it provides the cursors, the codecs, and the framers. Nothing allocates on
the encode or decode path. Nothing pulls in an async runtime. Nothing forces
a `serde`-style derive. You can drop it into an embedded firmware build, a
WebAssembly module, a kernel-bypass network stack, or a synchronous CLI tool,
and the public API is exactly the same.

If you are building a network protocol crate, a binary serialization layer,
or a framing adapter for some existing transport, this is intended to sit
underneath that work.

---

## Features

- **Zero-copy I/O.** [`ReadBuf`] and [`WriteBuf`] are cursors over borrowed
  byte slices. Reads return sub-slices that borrow from the input; writes
  go straight into the caller's buffer.
- **Codec trait pair.** [`Encode`] and [`Decode<'de>`] are minimal,
  allocation-free, and composable. The `'de` lifetime keeps borrowed
  payloads borrowed all the way through decoding.
- **LEB128 varint.** Unsigned varint for `u32` and `u64` with explicit
  overflow detection, compatible with Protocol Buffers, WebAssembly, and
  DWARF.
- **Zigzag.** Signed-to-unsigned mapping so negative deltas produce short
  varints. `const fn` end-to-end.
- **Bit-level cursors.** [`BitReader`] and [`BitWriter`] pack arbitrary-
  width fields, MSB-first, into and out of a caller-supplied byte buffer.
- **Framing strategies.** Stateless [`LengthPrefixed`] framer (u8 / u16 /
  u32 width, big or little endian, configurable payload cap) and
  [`Delimited`] framer (single- or multi-byte delimiters with a bounded
  scan window). Both implement a common [`Framer`] trait, so callers can
  swap strategies without rewriting protocol code.
- **`no_std` by default.** The `std` feature only enables an
  `impl std::error::Error for Error`. Drop it for embedded targets.
- **Zero runtime dependencies.** The full crate compiles standalone.
- **Stable API.** From 1.0 onward, the public surface is frozen.
  Subsequent 1.x releases are bug fixes and non-breaking additions.

---

## Quick start

```toml
[dependencies]
wire-codec = "1"
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

The full reference lives in [`docs/API.md`](docs/API.md). Runnable
demonstrations are under [`examples/`](examples/):

```sh
cargo run --example length_prefixed_echo
cargo run --example newline_protocol
cargo run --example varint_record
```

---

## Usage patterns

### Length-prefixed framing

Each frame is preceded by a fixed-width unsigned integer giving the payload
length. The width and byte order are chosen at construction.

```rust
use wire_codec::WriteBuf;
use wire_codec::framing::{Endian, Framer, LengthPrefixed, LengthWidth};

// 16-bit big-endian length, capped at 4 KiB per frame.
let framer = LengthPrefixed::new(LengthWidth::U16, Endian::Big)
    .with_max_payload(4096);

let mut out = [0u8; 64];
let mut buf = WriteBuf::new(&mut out);
framer.write_frame(b"hello", &mut buf).unwrap();

let frame = framer.next_frame(&out[..buf.position()]).unwrap().unwrap();
assert_eq!(frame.payload(), b"hello");
assert_eq!(frame.consumed(), 7); // 2-byte header + 5-byte payload
```

### Delimited (line-oriented) framing

Each frame is terminated by a configured delimiter byte sequence. The
delimiter is stripped from the emitted payload but counted in
`Frame::consumed`.

```rust
use wire_codec::framing::{Delimited, Framer};

let framer = Delimited::new(b"\r\n").unwrap();
let request = b"GET /index.html HTTP/1.1\r\nHost: example.com\r\n\r\nbody";

let line = framer.next_frame(request).unwrap().unwrap();
assert_eq!(line.payload(), b"GET /index.html HTTP/1.1");
assert_eq!(line.consumed(), 26);
```

### Compact integers (varint + zigzag)

`varint` encodes any `u32` / `u64` in one to ten bytes. Pair it with
`zigzag` to encode signed values whose typical magnitude is small.

```rust
use wire_codec::{WriteBuf, varint, zigzag};

let mut out = [0u8; 16];
let mut buf = WriteBuf::new(&mut out);

// Encode the timestamp delta -750 as a 2-byte varint.
varint::encode_u64(zigzag::encode_i64(-750), &mut buf).unwrap();
assert_eq!(buf.position(), 2);
```

### Bit-packed records

`BitReader` and `BitWriter` give you bit-level access for protocols where
multiple fields share a byte.

```rust
use wire_codec::BitWriter;

let mut storage = [0u8; 1];
let mut w = BitWriter::new(&mut storage);
w.write_bits(0b101, 3).unwrap();    // 3-bit type tag
w.write_bits(0b01100, 5).unwrap();  // 5-bit value
assert_eq!(w.finish(), 1);
assert_eq!(storage[0], 0xAC);
```

### Custom framing

Implement [`Framer`] on your own type to plug in a custom framing strategy
without giving up the rest of the toolkit.

```rust,no_run
use wire_codec::framing::{Frame, Framer};
use wire_codec::{Result, WriteBuf};

struct MyFramer;

impl Framer for MyFramer {
    fn next_frame<'a>(&self, input: &'a [u8]) -> Result<Option<Frame<'a>>> {
        // Your protocol-specific frame extraction logic.
        Ok(None)
    }
    fn write_frame(&self, payload: &[u8], out: &mut WriteBuf<'_>) -> Result<()> {
        out.write_bytes(payload)
    }
}
```

---

## Performance

Reference numbers measured on the development machine (release profile,
single thread, Windows 11). Variation across machines is expected; the
numbers are a baseline for regression detection, not a benchmark target.

| Operation                                       | ns/op   |
| ----------------------------------------------- | ------- |
| `varint::encode_u32` (small value)              | ~0.4    |
| `varint::encode_u32` (`u32::MAX`)               | ~0.7    |
| `varint::decode_u32` (2 bytes)                  | ~0.2    |
| `varint::decode_u32` (5 bytes)                  | ~0.2    |
| `varint::encode_u64` (`u64::MAX`)               | ~1.1    |
| `varint::decode_u64` (10 bytes)                 | ~1.8    |
| `zigzag::encode_i32`                            | ~0.4    |
| `ReadBuf::read_u32_be`                          | ~1.4    |
| `WriteBuf::write_u32_be`                        | ~1.5    |
| `LengthPrefixed::write_frame` (64 B)            | ~1.6    |
| `LengthPrefixed::next_frame` (64 B)             | ~2.0    |
| `Delimited::next_frame` (short line)            | ~2.4    |
| `Delimited::next_frame` (1 KiB scan + CRLF)     | ~1300   |

Reproduce locally:

```sh
cargo bench --bench codec
cargo bench --bench framing
```

The bench harness is a manual `std::time::Instant` loop with `black_box`
markers — zero dependencies and MSRV-compatible.

---

## Stability and semver

From `1.0.0` onward, the public API surface is frozen. Subsequent `1.x`
releases will only contain:

- Bug fixes (PATCH).
- Performance improvements that do not change observable behavior (PATCH).
- New additions that do not break existing code: new error variants (the
  `Error` enum is `#[non_exhaustive]`), new prefix widths (`LengthWidth`
  is `#[non_exhaustive]`), new helper methods on existing types,
  additional features behind feature flags (MINOR).

Any breaking change requires a `2.0.0` release. The crate follows
[Keep a Changelog](https://keepachangelog.com) and
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## `no_std` and feature flags

| Feature | Default | Effect |
| ------- | ------- | ------ |
| `std`   | enabled | Adds `impl std::error::Error for Error`. The crate works on `no_std` targets when this feature is disabled. |

To use the crate without `std`:

```toml
[dependencies]
wire-codec = { version = "1", default-features = false }
```

---

## Project layout

```
wire-codec/
├── src/
│   ├── lib.rs          # crate root, re-exports
│   ├── error.rs        # Error, Result
│   ├── buf.rs          # ReadBuf, WriteBuf
│   ├── traits.rs       # Encode, Decode
│   ├── varint.rs       # LEB128
│   ├── zigzag.rs       # signed-to-unsigned mapping
│   ├── bitfield.rs     # BitReader, BitWriter
│   └── framing/
│       ├── mod.rs      # Framer, Frame
│       ├── length.rs   # LengthPrefixed
│       └── delimiter.rs # Delimited
├── tests/              # smoke, integration, proptest suites
├── benches/            # codec.rs, framing.rs (manual harness)
├── examples/           # runnable demos
├── docs/
│   ├── API.md          # full API reference
│   └── release/        # per-version release notes
├── REPS.md             # engineering standards (supreme authority)
└── deny.toml           # supply-chain policy for cargo-deny
```

---

## Standards

- **REPS** governs every decision. See [`REPS.md`](REPS.md).
- **MSRV:** Rust 1.75.
- **Edition:** 2021.
- **Cross-platform:** Linux, macOS, Windows (CI on all three).
- **Supply chain:** `cargo audit` and `cargo deny` enforced in CI.
- **Test discipline:** unit, integration, property, and doctests.

---

## License

Dual-licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT License ([LICENSE-MIT](LICENSE-MIT))

at your option.

Contributions are accepted under the same dual license.


<!-- FOOT COPYRIGHT
################################################# -->
<div align="center">
  <h2></h2>
  <sup>COPYRIGHT <small>&copy;</small> 2026 <strong>JAMES GOBER.</strong></sup>
</div>
