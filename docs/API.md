<h1 align="center">
    <img width="99" alt="Rust logo" src="https://raw.githubusercontent.com/jamesgober/rust-collection/72baabd71f00e14aa9184efcb16fa3deddda3a0a/assets/rust-logo.svg">
    <br><b>wire-codec</b><br>
    <sub><sup>API REFERENCE</sup></sub>
</h1>

`wire-codec` is a runtime-agnostic binary frame and codec toolkit. Every public
item is listed below with its signature, behaviour, and at least one runnable
example. The crate is `no_std`-compatible when built with `--no-default-features`;
the `std` feature only adds an `impl std::error::Error for Error`.

```toml
[dependencies]
wire-codec = "0.9"
```

---

## Table of contents

- [Overview](#overview)
- [Error handling](#error-handling)
  - [`Error`](#error)
  - [`Result<T>`](#result)
- [Buffer cursors](#buffer-cursors)
  - [`ReadBuf`](#readbuf)
  - [`WriteBuf`](#writebuf)
- [Codec traits](#codec-traits)
  - [`Encode`](#encode)
  - [`Decode`](#decode)
- [Varint encoding](#varint-encoding)
  - [`varint::MAX_LEN_U16` / `MAX_LEN_U32` / `MAX_LEN_U64`](#varint-constants)
  - [`varint::encoded_len_u32`](#varintencoded_len_u32)
  - [`varint::encoded_len_u64`](#varintencoded_len_u64)
  - [`varint::encode_u32`](#varintencode_u32)
  - [`varint::encode_u64`](#varintencode_u64)
  - [`varint::decode_u32`](#varintdecode_u32)
  - [`varint::decode_u64`](#varintdecode_u64)
- [Zigzag encoding](#zigzag-encoding)
  - [`zigzag::encode_i32` / `decode_i32`](#zigzagencode_i32--decode_i32)
  - [`zigzag::encode_i64` / `decode_i64`](#zigzagencode_i64--decode_i64)
- [Bitfield cursors](#bitfield-cursors)
  - [`BitReader`](#bitreader)
  - [`BitWriter`](#bitwriter)
  - [`MAX_BIT_WIDTH`](#max_bit_width)
- [Framing](#framing)
  - [`Frame`](#frame)
  - [`Framer`](#framer)
  - [`LengthWidth`](#lengthwidth)
  - [`Endian`](#endian)
  - [`LengthPrefixed`](#lengthprefixed)
  - [`Delimited`](#delimited)
- [Crate constants](#crate-constants)
  - [`VERSION`](#version)
- [Cargo features](#cargo-features)

---

## Overview

Encoders accept any `&mut WriteBuf<'_>` and never allocate. Decoders accept any
`&mut ReadBuf<'de>` and return values that borrow from the input where
possible. Framing strategies are stateless; you call them once per frame and
advance your own input cursor by [`Frame::consumed`](#frame).

A typical pipeline is:

1. Build a `Framer` (length-prefixed or delimited).
2. Encode your payload type via `Encode::encode` into a `WriteBuf`.
3. Call `framer.write_frame(payload, &mut wire_buf)` to wrap it.
4. On the read side, call `framer.next_frame(input)` to extract the payload,
   then `T::decode(&mut ReadBuf::new(frame.payload()))` to deserialize.

---

## Error handling

### `Error`

```rust
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    UnexpectedEof,
    BufferFull,
    VarintOverflow,
    FrameTooLarge { len: usize, limit: usize },
    InvalidLengthPrefix,
    DelimiterNotFound,
    InvalidEncoding,
    BitOverflow,
}
```

| Variant | Meaning |
| ------- | ------- |
| `UnexpectedEof` | Reader ran out of bytes before the value completed. |
| `BufferFull` | Writer ran out of capacity. |
| `VarintOverflow` | A varint exceeded the maximum length for its target width, or its final byte carried bits above the target's bit width. |
| `FrameTooLarge { len, limit }` | A frame's declared payload length exceeds the framer's configured upper bound. |
| `InvalidLengthPrefix` | A length prefix is structurally invalid. |
| `DelimiterNotFound` | A delimiter byte sequence was not found within the configured search window. |
| `InvalidEncoding` | Decoded bytes violated a structural invariant of the codec. |
| `BitOverflow` | A bit-level call requested zero bits, more than [`MAX_BIT_WIDTH`](#max_bit_width), or a value that did not fit in the requested width. |
| `EmptyDelimiter` | A delimited-framer constructor was given an empty delimiter. |

`Error` implements `core::fmt::Display`. With the `std` feature it also
implements `std::error::Error`.

```rust
use wire_codec::{Error, ReadBuf};

let mut buf = ReadBuf::new(&[0x01]);
let result = buf.read_u16_be();
assert!(matches!(result, Err(Error::UnexpectedEof)));
```

### `Result<T>`

```rust
pub type Result<T> = core::result::Result<T, Error>;
```

Used as the return type of every fallible function in the crate.

---

## Buffer cursors

### `ReadBuf`

```rust
pub struct ReadBuf<'a> { /* ... */ }
```

A cursor over an immutable byte slice with a tracked read position. Decoders
borrow from the underlying slice, so reads never copy.

| Method | Description |
| ------ | ----------- |
| `pub const fn new(bytes: &'a [u8]) -> Self` | Wrap `bytes` and position at offset zero. |
| `pub const fn position(&self) -> usize` | Current read offset. |
| `pub const fn remaining(&self) -> usize` | Bytes left to read. |
| `pub const fn is_empty(&self) -> bool` | `true` if no bytes remain. |
| `pub fn remaining_slice(&self) -> &'a [u8]` | Borrow the unconsumed tail. |
| `pub fn peek(&self, n: usize) -> Result<&'a [u8]>` | Look at the next `n` bytes without advancing. |
| `pub fn read_bytes(&mut self, n: usize) -> Result<&'a [u8]>` | Read and advance. |
| `pub fn advance(&mut self, n: usize) -> Result<()>` | Skip without returning data. |
| `pub fn read_u8(&mut self) -> Result<u8>` | Read one byte. |
| `pub fn read_u16_be / read_u16_le` | Read a `u16` in big or little endian. |
| `pub fn read_u32_be / read_u32_le` | Read a `u32`. |
| `pub fn read_u64_be / read_u64_le` | Read a `u64`. |

```rust
use wire_codec::ReadBuf;

let mut buf = ReadBuf::new(&[0xDE, 0xAD, 0xBE, 0xEF]);
assert_eq!(buf.read_u16_be().unwrap(), 0xDEAD);
assert_eq!(buf.read_u16_be().unwrap(), 0xBEEF);
assert!(buf.is_empty());
```

### `WriteBuf`

```rust
pub struct WriteBuf<'a> { /* ... */ }
```

A cursor over a mutable byte slice with a tracked write position.

| Method | Description |
| ------ | ----------- |
| `pub fn new(bytes: &'a mut [u8]) -> Self` | Wrap `bytes` and position at offset zero. |
| `pub fn capacity(&self) -> usize` | Total bytes in the backing slice. |
| `pub fn position(&self) -> usize` | Bytes written so far. |
| `pub fn remaining(&self) -> usize` | Bytes left to write. |
| `pub fn written(&self) -> &[u8]` | Borrow the prefix that has been written. |
| `pub fn write_bytes(&mut self, src: &[u8]) -> Result<()>` | Append a slice. |
| `pub fn write_u8(&mut self, value: u8) -> Result<()>` | Write one byte. |
| `pub fn write_u16_be / write_u16_le` | Write a `u16`. |
| `pub fn write_u32_be / write_u32_le` | Write a `u32`. |
| `pub fn write_u64_be / write_u64_le` | Write a `u64`. |
| `pub fn into_written(self) -> &'a mut [u8]` | Consume the cursor; borrow the written prefix back. |

```rust
use wire_codec::WriteBuf;

let mut storage = [0u8; 4];
let mut buf = WriteBuf::new(&mut storage);
buf.write_u32_be(0xCAFEBABE).unwrap();
assert_eq!(&storage, &[0xCA, 0xFE, 0xBA, 0xBE]);
```

---

## Codec traits

### `Encode`

```rust
pub trait Encode {
    fn encoded_size(&self) -> usize;
    fn encode(&self, buf: &mut WriteBuf<'_>) -> Result<()>;
}
```

`encoded_size` must report the exact number of bytes `encode` will write on
success. Pairing the two enables size-aware framers to pre-reserve space.

```rust
use wire_codec::{Encode, Result, WriteBuf};

struct Tagged { tag: u8, value: u32 }

impl Encode for Tagged {
    fn encoded_size(&self) -> usize { 5 }
    fn encode(&self, buf: &mut WriteBuf<'_>) -> Result<()> {
        buf.write_u8(self.tag)?;
        buf.write_u32_be(self.value)
    }
}
```

A blanket impl is provided for `u8`.

### `Decode`

```rust
pub trait Decode<'de>: Sized {
    fn decode(buf: &mut ReadBuf<'de>) -> Result<Self>;
}
```

The lifetime `'de` ties the decoded value to the input slice, so types that
borrow (`&'de [u8]`, `&'de str`) can be returned without copying.

```rust
use wire_codec::{Decode, ReadBuf, Result};

struct Tagged { tag: u8, value: u32 }

impl<'de> Decode<'de> for Tagged {
    fn decode(buf: &mut ReadBuf<'de>) -> Result<Self> {
        let tag = buf.read_u8()?;
        let value = buf.read_u32_be()?;
        Ok(Self { tag, value })
    }
}
```

A blanket impl is provided for `u8`.

---

## Varint encoding

LEB128 unsigned varint. The low seven bits of each byte carry payload; the top
bit signals continuation. Compatible with Protocol Buffers, WebAssembly, and
DWARF.

### Varint constants

```rust
pub const MAX_LEN_U16: usize = 3;
pub const MAX_LEN_U32: usize = 5;
pub const MAX_LEN_U64: usize = 10;
```

Maximum encoded byte length for the corresponding target width.

### `varint::encoded_len_u32`

```rust
pub const fn encoded_len_u32(value: u32) -> usize;
```

Reports the number of bytes a `u32` value occupies once varint-encoded
(`1..=5`).

```rust
use wire_codec::varint;
assert_eq!(varint::encoded_len_u32(0), 1);
assert_eq!(varint::encoded_len_u32(127), 1);
assert_eq!(varint::encoded_len_u32(128), 2);
assert_eq!(varint::encoded_len_u32(u32::MAX), 5);
```

### `varint::encoded_len_u64`

```rust
pub const fn encoded_len_u64(value: u64) -> usize;
```

Reports the number of bytes a `u64` value occupies once varint-encoded
(`1..=10`).

### `varint::encode_u32`

```rust
pub fn encode_u32(value: u32, buf: &mut WriteBuf<'_>) -> Result<()>;
```

Encodes `value` into `buf`. Returns `Error::BufferFull` when the cursor lacks
capacity for `encoded_len_u32(value)` bytes.

```rust
use wire_codec::{varint, WriteBuf};

let mut out = [0u8; 5];
let mut buf = WriteBuf::new(&mut out);
varint::encode_u32(300, &mut buf).unwrap();
assert_eq!(buf.written(), &[0xAC, 0x02]);
```

### `varint::encode_u64`

```rust
pub fn encode_u64(value: u64, buf: &mut WriteBuf<'_>) -> Result<()>;
```

Same contract as `encode_u32`, extended to `u64`.

### `varint::decode_u32`

```rust
pub fn decode_u32(buf: &mut ReadBuf<'_>) -> Result<u32>;
```

Decodes a varint and validates that its value fits in `u32`. Returns:

- `Error::UnexpectedEof` if the cursor is exhausted mid-varint.
- `Error::VarintOverflow` if more than `MAX_LEN_U32` bytes are consumed, or
  if the final byte carries bits above the `u32` window.

```rust
use wire_codec::{varint, ReadBuf};

let mut buf = ReadBuf::new(&[0xAC, 0x02]);
assert_eq!(varint::decode_u32(&mut buf).unwrap(), 300);
```

### `varint::decode_u64`

```rust
pub fn decode_u64(buf: &mut ReadBuf<'_>) -> Result<u64>;
```

Same contract as `decode_u32`, extended to `u64`.

---

## Zigzag encoding

Maps signed integers to unsigned ones so that small-magnitude negative values
produce small-magnitude varints. Pair these helpers with the [varint](#varint-encoding)
functions to encode signed values compactly.

| signed | unsigned |
| ------ | -------- |
|  0 | 0 |
| -1 | 1 |
|  1 | 2 |
| -2 | 3 |
|  2 | 4 |

### `zigzag::encode_i32` / `decode_i32`

```rust
pub const fn encode_i32(value: i32) -> u32;
pub const fn decode_i32(value: u32) -> i32;
```

```rust
use wire_codec::zigzag;
assert_eq!(zigzag::encode_i32(-1), 1);
assert_eq!(zigzag::decode_i32(1), -1);
assert_eq!(zigzag::encode_i32(i32::MIN), u32::MAX);
```

### `zigzag::encode_i64` / `decode_i64`

```rust
pub const fn encode_i64(value: i64) -> u64;
pub const fn decode_i64(value: u64) -> i64;
```

---

## Bitfield cursors

Bit-level read and write cursors with most-significant-bit-first ordering.
Useful for packed protocols where multiple fields share a single byte.

### `BitReader`

```rust
pub struct BitReader<'a> { /* ... */ }
```

| Method | Description |
| ------ | ----------- |
| `pub const fn new(bytes: &'a [u8]) -> Self` | Wrap a byte slice. |
| `pub const fn bits_consumed(&self) -> usize` | Bits read so far. |
| `pub const fn bits_remaining(&self) -> usize` | Bits still available. |
| `pub fn align_to_byte(&mut self)` | Skip to the next byte boundary. |
| `pub fn read_bits(&mut self, n: u32) -> Result<u64>` | Read `1..=64` bits into the low end of a `u64`. |

```rust
use wire_codec::BitReader;

let mut r = BitReader::new(&[0xAC]); // 0b1010_1100
assert_eq!(r.read_bits(3).unwrap(), 0b101);
assert_eq!(r.read_bits(5).unwrap(), 0b01100);
```

### `BitWriter`

```rust
pub struct BitWriter<'a> { /* ... */ }
```

| Method | Description |
| ------ | ----------- |
| `pub fn new(bytes: &'a mut [u8]) -> Self` | Wrap a mutable byte slice. |
| `pub fn bits_written(&self) -> usize` | Bits written so far. |
| `pub fn align_to_byte(&mut self) -> Result<()>` | Zero-pad to the next byte boundary. |
| `pub fn write_bits(&mut self, value: u64, n: u32) -> Result<()>` | Write the low `n` bits of `value`. |
| `pub fn finish(self) -> usize` | Consume the cursor; return whole-byte count, rounding any partial trailing byte up. |

```rust
use wire_codec::BitWriter;

let mut storage = [0u8; 1];
let mut w = BitWriter::new(&mut storage);
w.write_bits(0b101, 3).unwrap();
w.write_bits(0b01100, 5).unwrap();
assert_eq!(w.finish(), 1);
assert_eq!(storage[0], 0xAC);
```

### `MAX_BIT_WIDTH`

```rust
pub const MAX_BIT_WIDTH: u32 = 64;
```

Upper bound on `n` for both `read_bits` and `write_bits`.

---

## Framing

A frame is a delimited unit of bytes on the wire. The `Framer` trait defines a
stateless extraction strategy; two concrete strategies ship with the crate.

### `Frame`

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Frame<'a> { /* ... */ }

impl<'a> Frame<'a> {
    pub const fn new(payload: &'a [u8], consumed: usize) -> Self;
    pub const fn payload(&self) -> &'a [u8];
    pub const fn consumed(&self) -> usize;
}
```

Carries the zero-copy payload borrow alongside the byte count the framer
consumed (header / delimiter included). Advance your input cursor by
`consumed` after each call to `next_frame`.

### `Framer`

```rust
pub trait Framer {
    fn next_frame<'a>(&self, input: &'a [u8]) -> Result<Option<Frame<'a>>>;
    fn write_frame(&self, payload: &[u8], out: &mut WriteBuf<'_>) -> Result<()>;
}
```

`next_frame` returns `Ok(Some(frame))` when a complete frame is available,
`Ok(None)` when more bytes are needed, and `Err(_)` when the input violates the
protocol.

### `LengthWidth`

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LengthWidth { U8, U16, U32 }

impl LengthWidth {
    pub const fn header_size(self) -> usize;
    pub const fn max_payload(self) -> u64;
}
```

Width of the length prefix and the largest payload size the prefix can encode.

### `Endian`

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Endian { Big, Little }
```

Byte order for the length prefix.

### `LengthPrefixed`

```rust
pub struct LengthPrefixed { /* ... */ }

impl LengthPrefixed {
    pub const fn new(width: LengthWidth, endian: Endian) -> Self;
    pub const fn with_max_payload(self, max: u64) -> Self;
    pub const fn width(&self) -> LengthWidth;
    pub const fn endian(&self) -> Endian;
    pub const fn max_payload(&self) -> u64;
}
```

Each frame is preceded by an unsigned integer giving the payload length in
bytes. `with_max_payload` clamps to `LengthWidth::max_payload(self.width)`.

```rust
use wire_codec::WriteBuf;
use wire_codec::framing::{Endian, Framer, LengthPrefixed, LengthWidth};

let framer = LengthPrefixed::new(LengthWidth::U16, Endian::Big);

let mut out = [0u8; 16];
let mut buf = WriteBuf::new(&mut out);
framer.write_frame(b"hello", &mut buf).unwrap();
let n = buf.position();
assert_eq!(&out[..n], &[0x00, 0x05, b'h', b'e', b'l', b'l', b'o']);

let frame = framer.next_frame(&out[..n]).unwrap().unwrap();
assert_eq!(frame.payload(), b"hello");
assert_eq!(frame.consumed(), 7);
```

### `Delimited`

```rust
pub struct Delimited<'d> { /* ... */ }

impl<'d> Delimited<'d> {
    pub const fn new(delimiter: &'d [u8]) -> Result<Self, Error>;
    pub const fn with_max_payload(self, max: usize) -> Self;
    pub const fn delimiter(&self) -> &'d [u8];
    pub const fn max_payload(&self) -> usize;
}
```

Frames are separated by a byte sequence (one or more bytes). The delimiter is
stripped from the emitted payload but counted toward `Frame::consumed`. An
empty delimiter yields `Error::EmptyDelimiter` rather than panicking.

```rust
use wire_codec::framing::{Delimited, Framer};

let framer = Delimited::new(b"\r\n").unwrap();
let frame = framer.next_frame(b"GET /\r\nrest").unwrap().unwrap();
assert_eq!(frame.payload(), b"GET /");
assert_eq!(frame.consumed(), 7);
```

When `max_payload` is set, the framer caps its delimiter search at that range
and returns `Error::FrameTooLarge` rather than scanning unbounded input.

---

## Crate constants

### `VERSION`

```rust
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
```

Crate version string, populated at build time. Equal to `"0.9.0"` for this
release.

---

## Cargo features

| Feature | Default | Effect |
| ------- | ------- | ------ |
| `std`   | yes     | Adds `impl std::error::Error for Error`. Drop this feature to use the crate in `no_std` environments. |

The crate has zero runtime dependencies.

---

## Testing and benchmarks

The repository ships with:

- **Unit tests** alongside each module (`src/**/tests`).
- **Integration tests** in `tests/`:
  - `smoke.rs` — minimal cross-module pipeline check.
  - `integration_protocols.rs` — realistic protocol shapes (chunked
    length-prefixed streams, newline and CR/LF text protocols, two-layer
    framed binary records, back-pressure semantics).
- **Property tests** (`proptest`) in `tests/prop_*.rs` covering varint
  round-trip and panic-freedom on adversarial input, zigzag bijection,
  framer round-trip across all `LengthWidth`/`Endian` combinations,
  bitfield round-trip across arbitrary `(value, width)` sequences.
- **Benchmarks** in `benches/`:
  - `codec.rs` — varint, zigzag, and buffer-primitive throughput.
  - `framing.rs` — length-prefixed and delimited framer cost.

Run them with:

```sh
cargo test --all-features
cargo bench --bench codec
cargo bench --bench framing
```

---

## End-to-end example

Length-prefix a varint-encoded record and decode it back, end to end:

```rust
use wire_codec::framing::{Endian, Framer, LengthPrefixed, LengthWidth};
use wire_codec::{Decode, Encode, ReadBuf, Result, WriteBuf, varint, zigzag};

#[derive(PartialEq, Eq, Debug)]
struct Record<'a> { delta: i64, payload: &'a [u8] }

impl<'a> Encode for Record<'a> {
    fn encoded_size(&self) -> usize {
        varint::encoded_len_u64(zigzag::encode_i64(self.delta))
            + varint::encoded_len_u64(self.payload.len() as u64)
            + self.payload.len()
    }
    fn encode(&self, buf: &mut WriteBuf<'_>) -> Result<()> {
        varint::encode_u64(zigzag::encode_i64(self.delta), buf)?;
        varint::encode_u64(self.payload.len() as u64, buf)?;
        buf.write_bytes(self.payload)
    }
}

impl<'de> Decode<'de> for Record<'de> {
    fn decode(buf: &mut ReadBuf<'de>) -> Result<Self> {
        let delta = zigzag::decode_i64(varint::decode_u64(buf)?);
        let len = varint::decode_u64(buf)? as usize;
        let payload = buf.read_bytes(len)?;
        Ok(Self { delta, payload })
    }
}

let record = Record { delta: -1234, payload: b"hello world" };

let mut inner = [0u8; 32];
let mut inner_buf = WriteBuf::new(&mut inner);
record.encode(&mut inner_buf).unwrap();
let inner_len = inner_buf.position();

let framer = LengthPrefixed::new(LengthWidth::U16, Endian::Big);
let mut wire = [0u8; 64];
let mut wire_buf = WriteBuf::new(&mut wire);
framer.write_frame(&inner[..inner_len], &mut wire_buf).unwrap();
let wire_len = wire_buf.position();

let frame = framer.next_frame(&wire[..wire_len]).unwrap().unwrap();
let mut read = ReadBuf::new(frame.payload());
let decoded = Record::decode(&mut read).unwrap();
assert_eq!(decoded, record);
```
