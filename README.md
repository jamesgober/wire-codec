<h1 align="center">
    <img width="99" alt="Rust logo" src="https://raw.githubusercontent.com/jamesgober/rust-collection/72baabd71f00e14aa9184efcb16fa3deddda3a0a/assets/rust-logo.svg">
    <br>
    <strong>wire-codec</strong>
    <br>
    <sup><sub>BINARY FRAMING AND ENCODING PRIMITIVES</sub></sup>
</h1>

<p align="center">
    <a href="https://crates.io/crates/wire-codec"><img alt="crates.io" src="https://img.shields.io/crates/v/wire-codec.svg"></a>
    <a href="https://crates.io/crates/wire-codec"><img alt="downloads" src="https://img.shields.io/crates/d/wire-codec.svg"></a>
    <a href="https://docs.rs/wire-codec"><img alt="docs.rs" src="https://docs.rs/wire-codec/badge.svg"></a>
    <a href="https://github.com/rust-lang/rfcs/blob/master/text/2495-min-rust-version.md" title="MSRV"><img alt="MSRV" src="https://img.shields.io/badge/MSRV-1.85%2B-blue"></a>
    <a href="https://github.com/jamesgober/wire-codec/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/jamesgober/wire-codec/actions/workflows/ci.yml/badge.svg"></a>
</p>

<p align="center"><b>A runtime-agnostic toolkit for binary framing and codec composition.</b></p>

<p align="center">
Zero-copy buffer cursors, LEB128 varint and zigzag integer codecs, MSB-first
bit cursors and bit-packed structs, CRC checksums, COBS byte stuffing, and
length-prefixed, varint-prefixed, and delimited frame extraction — all
allocation-free, all <code>no_std</code>-capable, all behind a small,
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
a `serde`-style data model. You can drop it into an embedded firmware build,
a WebAssembly module, a kernel-bypass network stack, or a synchronous CLI
tool, and the public API is exactly the same. When you do live in Tokio or
a futures-based runtime, optional adapters plug any framer into
`tokio_util::codec::Framed` or `asynchronous_codec::Framed`.

If you are building a network protocol crate, a binary serialization layer,
or a framing adapter for some existing transport, this is intended to sit
underneath that work.

---

## Features

- **Zero-copy I/O.** [`ReadBuf`] and [`WriteBuf`] are cursors over borrowed
  byte slices. Reads return sub-slices that borrow from the input; writes
  go straight into the caller's buffer. Every operation is atomic: on error
  the cursor has not moved.
- **Codec trait pair.** [`Encode`] and [`Decode<'de>`][`Decode`] are minimal,
  allocation-free, and composable, with impls for `u8`, `i8`, `bool`, byte
  arrays, tuples, and the [`Be`] / [`Le`] / [`Varint`] wrappers that make the
  byte order of every multi-byte integer explicit.
- **LEB128 varint.** `u16`, `u32`, `u64`, `usize`, and zigzag `i32` / `i64`,
  compatible with Protocol Buffers, WebAssembly, and DWARF. Canonical
  decoders reject overlong encodings for signature- and hash-sensitive
  protocols.
- **Zigzag.** Signed-to-unsigned mapping so negative deltas produce short
  varints. `const fn` end-to-end.
- **Bit-level cursors and packed structs.** [`BitReader`] and [`BitWriter`]
  pack arbitrary-width fields (signed or unsigned), MSB-first, with padding
  bits always zeroed. [`packed_struct!`] declares a struct with per-field bit
  widths, checked at compile time, and implements `Encode` / `Decode` for it.
- **Framing strategies**, all behind one [`Framer`] trait:
  - [`LengthPrefixed`]: 1, 2, 3, 4, or 8 byte prefix, either byte order,
    optional length adjustment (for "length includes header" protocols).
  - [`VarintPrefixed`]: protobuf-style varint length prefix, bounded and
    canonical by default.
  - [`Delimited`]: single- or multi-byte delimiters, a fast SWAR search
    (or `memchr` with the optional feature), and refusal to write payloads
    that would split on the receiving side.
  - [`Checksummed`]: appends and verifies a CRC-16, CRC-32, CRC-32C, or
    custom [`Checksum`] after the frames of any other framer.
- **Linear-time streaming.** [`StreamDecoder`] resumes a delimiter search
  where the previous call stopped, so a peer that drips one byte at a time
  costs linear, not quadratic, CPU.
- **COBS.** Allocation-free [`cobs`] encode, decode, and in-place decode for
  zero-delimited serial framing.
- **Hardened for hostile input.** Checked arithmetic on every length taken
  from the wire (no panics on 32-bit targets), bounded constructors for
  untrusted peers, fuzz targets for every decoder.
- **`no_std`.** Disable the default `std` feature and the whole core,
  including the error type's `core::error::Error` impl, builds for bare-metal
  targets.
- **No required dependencies.** Every integration (`memchr`, `bytes`,
  `tokio-util`, `asynchronous-codec`) is an optional, off-by-default feature.
- **Stable API.** From 1.0 onward, the public surface is frozen. 1.x
  releases are bug fixes and non-breaking additions.

---

## Quick start

```toml
[dependencies]
wire-codec = "1.1"
```

```rust
use wire_codec::WriteBuf;
use wire_codec::framing::{Endian, Framer, LengthPrefixed, LengthWidth};

let framer = LengthPrefixed::bounded(LengthWidth::U16, Endian::Big);

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

// `buf` borrows `out` mutably, so read the frame back through `written()`.
let frame = framer.next_frame(buf.written()).unwrap().unwrap();
assert_eq!(frame.payload(), b"hello");
assert_eq!(frame.consumed(), 7); // 2-byte header + 5-byte payload
```

Protocols whose length field counts the header itself use a length
adjustment: `.with_length_adjustment(-2)` for a 2-byte self-counting prefix.

### Varint length-prefixed framing

The length-delimited stream format of Protocol Buffers: a LEB128 length,
then the payload. Bounded (8 MiB) and canonical by default.

```rust
use wire_codec::WriteBuf;
use wire_codec::framing::{Framer, VarintPrefixed};

let framer = VarintPrefixed::new();
let mut out = [0u8; 16];
let mut buf = WriteBuf::new(&mut out);
framer.write_frame(b"hello", &mut buf).unwrap();
assert_eq!(buf.written(), b"\x05hello");
```

### Delimited (line-oriented) framing

Each frame is terminated by a configured delimiter byte sequence. The
delimiter is stripped from the emitted payload but counted in
`Frame::consumed`.

```rust
use wire_codec::framing::{Delimited, Framer};

let framer = Delimited::bounded(b"\r\n").unwrap();
let request = b"GET /index.html HTTP/1.1\r\nHost: example.com\r\n\r\nbody";

let line = framer.next_frame(request).unwrap().unwrap();
assert_eq!(line.payload(), b"GET /index.html HTTP/1.1");
assert_eq!(line.consumed(), 26);
```

### Streaming input

When bytes arrive in pieces, keep the framer in a [`StreamDecoder`]. It
remembers how far the previous call searched, so every byte is examined
about once no matter how the input is chunked.

```rust
use wire_codec::framing::{Delimited, StreamDecoder};

let mut decoder = StreamDecoder::new(Delimited::bounded(b"\n").unwrap());
let mut buffer: Vec<u8> = Vec::new();

for chunk in [&b"PI"[..], b"NG\nQU", b"IT\n"] {
    buffer.extend_from_slice(chunk);
    let mut start = 0;
    while let Some(frame) = decoder.decode(&buffer[start..]).unwrap() {
        println!("command: {:?}", frame.payload());
        start += frame.consumed();
    }
    buffer.drain(..start);
}
```

### Untrusted input

`LengthPrefixed::new` and `Delimited::new` keep their 1.0 behaviour and
accept payloads as large as the prefix can describe (4 GiB for a `u32`
prefix, unbounded for a delimiter). Anything reading from a peer you do not
control should use a limit:

| Constructor | Default limit |
| ----------- | ------------- |
| `LengthPrefixed::bounded(width, endian)` | 8 MiB (or the width's maximum, if smaller) |
| `VarintPrefixed::new()` | 8 MiB |
| `Delimited::bounded(delimiter)` | 64 KiB |
| any framer `.with_max_payload(n)` | `n` |

### Compact integers (varint + zigzag)

`varint` encodes integers in one to ten bytes. The signed helpers apply
zigzag first, so small negative values stay short.

```rust
use wire_codec::{ReadBuf, WriteBuf, varint};

let mut out = [0u8; 16];
let mut buf = WriteBuf::new(&mut out);

// Encode the timestamp delta -750 as a 2-byte varint.
varint::encode_i64(-750, &mut buf).unwrap();
assert_eq!(buf.position(), 2);

let mut read = ReadBuf::new(buf.written());
assert_eq!(varint::decode_i64(&mut read).unwrap(), -750);
```

### Typed fields

`Encode` and `Decode` compose through tuples and the byte-order wrappers.

```rust
use wire_codec::{Be, Decode, Encode, ReadBuf, Varint, WriteBuf};

let message = (0x01u8, Be(0xCAFE_u16), Varint(300u32), true);
let mut out = [0u8; 16];
let mut buf = WriteBuf::new(&mut out);
message.encode(&mut buf).unwrap();
assert_eq!(buf.written(), &[0x01, 0xCA, 0xFE, 0xAC, 0x02, 0x01]);

let decoded = <(u8, Be<u16>, Varint<u32>, bool)>::decode(&mut ReadBuf::new(buf.written())).unwrap();
assert_eq!(decoded, message);
```

### Bit-packed records

`packed_struct!` declares the layout once; widths are checked at compile
time and values at encode time.

```rust
use wire_codec::{Decode, Encode, ReadBuf, WriteBuf, packed_struct};

packed_struct! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Header {
        pub kind: u8 = 3,     // 3-bit type tag
        pub urgent: bool = 1, // 1-bit flag
        pub delta: i8 = 4,    // 4-bit signed delta
    }
}

let header = Header { kind: 5, urgent: true, delta: -3 };
let mut out = [0u8; Header::SIZE];
header.encode(&mut WriteBuf::new(&mut out)).unwrap();
assert_eq!(out, [0b101_1_1101]);
assert_eq!(Header::decode(&mut ReadBuf::new(&out)).unwrap(), header);
```

For one-off layouts, `BitWriter` and `BitReader` give direct bit-level
access:

```rust
use wire_codec::BitWriter;

let mut storage = [0u8; 1];
let mut w = BitWriter::new(&mut storage);
w.write_bits(0b101, 3).unwrap();    // 3-bit type tag
w.write_bits(0b01100, 5).unwrap();  // 5-bit value
assert_eq!(w.finish(), 1);
assert_eq!(storage[0], 0xAC);
```

### Checksummed frames

```rust
use wire_codec::checksum::Crc32;
use wire_codec::framing::{Checksummed, Endian, Framer, LengthPrefixed, LengthWidth};
use wire_codec::{Error, WriteBuf};

let framer = Checksummed::new(
    LengthPrefixed::bounded(LengthWidth::U16, Endian::Big),
    Crc32,
    Endian::Little,
);
let mut out = [0u8; 32];
let mut buf = WriteBuf::new(&mut out);
framer.write_frame(b"ping", &mut buf).unwrap();
let n = buf.position();

out[3] ^= 0x01; // a bit flips in transit
assert_eq!(framer.next_frame(&out[..n]), Err(Error::ChecksumMismatch));
```

### Tokio and futures adapters

With the `tokio-util` (or `asynchronous-codec`) feature, wrap any framer in
`FrameCodec` and hand it to `Framed`:

```rust,ignore
use tokio_util::codec::Framed;
use wire_codec::codec::FrameCodec;
use wire_codec::framing::{Endian, LengthPrefixed, LengthWidth};

let codec = FrameCodec::new(LengthPrefixed::bounded(LengthWidth::U32, Endian::Big));
let framed = Framed::new(tcp_stream, codec);
// Stream<Item = io::Result<BytesMut>> + Sink<&[u8]> / Sink<Bytes>
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
        let _ = input;
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
fat LTO, single thread, Windows 11, x86_64). Decode inputs pass through
`black_box`, so nothing is constant-folded. Variation across machines is
expected; the numbers are a baseline for regression detection, not a
benchmark target.

| Operation                                              | Time            |
| ------------------------------------------------------ | --------------- |
| `varint::encode_u32` (value 300)                       | 0.44 ns         |
| `varint::encode_u32` (`u32::MAX`)                      | 0.57 ns         |
| `varint::decode_u32` (2 bytes)                         | 0.49 ns         |
| `varint::decode_u32` (5 bytes)                         | 1.16 ns         |
| `varint::encode_u64` (`u64::MAX`)                      | 1.24 ns         |
| `varint::decode_u64` (10 bytes)                        | 2.75 ns         |
| `varint::decode_u64` (mixed 4096-value stream, per value) | 0.77 ns      |
| `zigzag::encode_i32`                                   | 0.26 ns         |
| `ReadBuf::read_u32_be` (16 reads)                      | 2.9 ns          |
| `WriteBuf::write_u32_be` (16 writes)                   | 1.6 ns          |
| `LengthPrefixed::write_frame` (64 B)                   | 1.25 ns         |
| `LengthPrefixed::next_frame` (64 B)                    | 0.47 ns         |
| `VarintPrefixed::next_frame` (64 B)                    | 0.92 ns         |
| `Checksummed<LengthPrefixed, Crc32>::next_frame` (64 B) | 19 ns          |
| `Delimited::next_frame` (short line)                   | 2.6 ns          |
| `Delimited::next_frame` (1 KiB scan + CRLF)            | 26 ns           |
| `Delimited::next_frame` (64 KiB scan + LF)             | 1.5 µs (~43 GB/s) |
| `StreamDecoder<Delimited>`, 64 KiB line fed 1 byte per call | 155 µs total |
| `checksum::crc32` / `crc32c` (1 KiB)                   | 0.33 µs (~3 GB/s) |
| `checksum::crc16_ccitt_false` (1 KiB)                  | 1.85 µs         |
| `cobs::encode` / `cobs::decode` (1 KiB)                | 0.64 / 0.54 µs  |

Numbers are the minimum of four runs. For comparison, 1.0 scanned a 1 KiB
line in about 1.8 µs and spent about 430 ms on the 64 KiB slow-drip case,
which is now linear.

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
  is `#[non_exhaustive]`), new helper methods on existing types, new
  provided trait methods, additional features behind feature flags (MINOR).

Any breaking change requires a `2.0.0` release. The crate follows
[Keep a Changelog](https://keepachangelog.com) and
[Semantic Versioning](https://semver.org/spec/v2.0.0.html).

**MSRV policy.** The minimum supported Rust version is 1.85. It is raised
only in a MINOR release, with the change called out in the changelog, and
never in a PATCH release. Users pinned to an older compiler can stay on the
previous minor line (`wire-codec = "~1.0"` for Rust 1.75). The edition 2024
resolver honours `rust-version`, so `cargo update` will not pick a release
your toolchain cannot build.

---

## `no_std` and feature flags

| Feature | Default | Effect |
| ------- | ------- | ------ |
| `std`   | enabled | Links the standard library and adds `From<Error> for std::io::Error`. The crate is `no_std` when this feature is disabled; `Error` implements `core::error::Error` either way. |
| `memchr` | off | Delimiter search through the [`memchr`](https://crates.io/crates/memchr) crate (SIMD kernels, linear-time multi-byte search). Works without `std`. |
| `bytes` | off | `codec::FrameCodec`, a [`bytes::BytesMut`](https://crates.io/crates/bytes) adapter for any framer. Works without `std` (needs `alloc`). |
| `tokio-util` | off | `tokio_util::codec::{Decoder, Encoder}` for `FrameCodec`. Implies `bytes` and `std`. |
| `asynchronous-codec` | off | `asynchronous_codec::{Decoder, Encoder}` for `FrameCodec`. Implies `bytes` and `std`. |

To use the crate without `std`:

```toml
[dependencies]
wire-codec = { version = "1.1", default-features = false }
```

---

## Project layout

```text
wire-codec/
├── src/
│   ├── lib.rs              # crate root, re-exports
│   ├── error.rs            # Error, Result
│   ├── buf.rs              # ReadBuf, WriteBuf
│   ├── traits.rs           # Encode, Decode, Be, Le, Varint
│   ├── varint.rs           # LEB128 (lenient and canonical)
│   ├── zigzag.rs           # signed-to-unsigned mapping
│   ├── bitfield.rs         # BitReader, BitWriter
│   ├── packed.rs           # packed_struct!, PackedField
│   ├── checksum.rs         # Checksum, CRC-16, CRC-32, CRC-32C
│   ├── cobs.rs             # COBS byte stuffing
│   ├── codec.rs            # FrameCodec (bytes / tokio-util / asynchronous-codec)
│   └── framing/
│       ├── mod.rs          # Framer, Frame, StreamDecoder
│       ├── length.rs       # LengthPrefixed
│       ├── varint_prefixed.rs # VarintPrefixed
│       ├── delimiter.rs    # Delimited
│       ├── checksummed.rs  # Checksummed
│       └── search.rs       # SWAR / memchr delimiter search
├── tests/                  # smoke, integration, proptest, fuzz-harness suites
├── fuzz/                   # cargo-fuzz targets (not published)
├── benches/                # codec.rs, framing.rs (manual harness)
├── examples/               # runnable demos
├── docs/
│   ├── API.md              # full API reference
│   └── release/            # per-version release notes
├── REPS.md                 # engineering standards (supreme authority)
└── deny.toml               # supply-chain policy for cargo-deny
```

---

## Standards

- **REPS** governs every decision. See [`REPS.md`](REPS.md).
- **MSRV:** Rust 1.85 (verified in CI on Linux, macOS, and Windows).
- **Edition:** 2024.
- **Cross-platform:** Linux, macOS, Windows (CI on all three), plus 32-bit
  (`i686`), `no_std` (`thumbv7em-none-eabihf`), and `wasm32` builds in CI.
- **Safety:** `#![forbid(unsafe_code)]`.
- **Supply chain:** `cargo audit` and `cargo deny` enforced in CI.
- **Test discipline:** unit, integration, property, doctests, and
  libFuzzer targets; README and API reference examples are compiled as
  doctests.

---

## License

Dual-licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT License ([LICENSE-MIT](LICENSE-MIT))

at your option.

Contributions are accepted under the same dual license.

[`ReadBuf`]: https://docs.rs/wire-codec/latest/wire_codec/buf/struct.ReadBuf.html
[`WriteBuf`]: https://docs.rs/wire-codec/latest/wire_codec/buf/struct.WriteBuf.html
[`Encode`]: https://docs.rs/wire-codec/latest/wire_codec/traits/trait.Encode.html
[`Decode`]: https://docs.rs/wire-codec/latest/wire_codec/traits/trait.Decode.html
[`Be`]: https://docs.rs/wire-codec/latest/wire_codec/traits/struct.Be.html
[`Le`]: https://docs.rs/wire-codec/latest/wire_codec/traits/struct.Le.html
[`Varint`]: https://docs.rs/wire-codec/latest/wire_codec/traits/struct.Varint.html
[`BitReader`]: https://docs.rs/wire-codec/latest/wire_codec/bitfield/struct.BitReader.html
[`BitWriter`]: https://docs.rs/wire-codec/latest/wire_codec/bitfield/struct.BitWriter.html
[`packed_struct!`]: https://docs.rs/wire-codec/latest/wire_codec/macro.packed_struct.html
[`Framer`]: https://docs.rs/wire-codec/latest/wire_codec/framing/trait.Framer.html
[`StreamDecoder`]: https://docs.rs/wire-codec/latest/wire_codec/framing/struct.StreamDecoder.html
[`LengthPrefixed`]: https://docs.rs/wire-codec/latest/wire_codec/framing/length/struct.LengthPrefixed.html
[`VarintPrefixed`]: https://docs.rs/wire-codec/latest/wire_codec/framing/varint_prefixed/struct.VarintPrefixed.html
[`Delimited`]: https://docs.rs/wire-codec/latest/wire_codec/framing/delimiter/struct.Delimited.html
[`Checksummed`]: https://docs.rs/wire-codec/latest/wire_codec/framing/checksummed/struct.Checksummed.html
[`Checksum`]: https://docs.rs/wire-codec/latest/wire_codec/checksum/trait.Checksum.html
[`cobs`]: https://docs.rs/wire-codec/latest/wire_codec/cobs/index.html


<!-- FOOT COPYRIGHT
################################################# -->
<div align="center">
  <h2></h2>
  <sup>COPYRIGHT <small>&copy;</small> 2026 <strong>JAMES GOBER.</strong></sup>
</div>
