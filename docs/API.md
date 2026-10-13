<h1 align="center">
    <img width="99" alt="Rust logo" src="https://raw.githubusercontent.com/jamesgober/rust-collection/72baabd71f00e14aa9184efcb16fa3deddda3a0a/assets/rust-logo.svg">
    <br><b>wire-codec</b><br>
    <sub><sup>API REFERENCE</sup></sub>
</h1>

`wire-codec` is a runtime-agnostic binary frame and codec toolkit. Every public
item is listed below with its signature and behaviour, with runnable examples
for each type, trait, and function group. Signature listings are marked `rust,ignore`; every other code block in this
file is compiled and run as a doctest (`cargo test --doc`), so the examples
cannot drift from the code.

The crate is `no_std`-compatible when built with `--no-default-features`.
See [Cargo features](#cargo-features) for the optional integrations.

```toml
[dependencies]
wire-codec = "1.1"
```

The API surface is **stable as of 1.0.0**. New `Error` variants and new
`LengthWidth` widths may appear in future minor releases (both enums are
`#[non_exhaustive]`), as may new methods, provided trait methods, and
feature-gated modules. Existing types, traits, function signatures, and
behaviour will not change without a major version bump. The MSRV (Rust
1.85) is raised only in minor releases and always called out in the
changelog.

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
  - [Provided implementations](#provided-implementations)
  - [`Be` / `Le` / `Varint`](#be--le--varint)
- [Varint encoding](#varint-encoding)
  - [Constants](#varint-constants)
  - [Encoded length](#encoded-length)
  - [Encoding](#encoding)
  - [Decoding](#decoding)
  - [Canonical decoding](#canonical-decoding)
- [Zigzag encoding](#zigzag-encoding)
- [Bitfield cursors](#bitfield-cursors)
  - [`BitReader`](#bitreader)
  - [`BitWriter`](#bitwriter)
  - [`MAX_BIT_WIDTH`](#max_bit_width)
- [Packed structs](#packed-structs)
  - [`packed_struct!`](#packed_struct)
  - [`PackedField`](#packedfield)
- [Framing](#framing)
  - [`Frame`](#frame)
  - [`Framer`](#framer)
  - [`StreamDecoder`](#streamdecoder)
  - [`LengthWidth`](#lengthwidth)
  - [`Endian`](#endian)
  - [`LengthPrefixed`](#lengthprefixed)
  - [`VarintPrefixed`](#varintprefixed)
  - [`Delimited`](#delimited)
  - [`Checksummed` / `ChecksumScope`](#checksummed--checksumscope)
- [Checksums](#checksums)
- [COBS](#cobs)
- [`bytes` / Tokio / futures adapters](#bytes--tokio--futures-adapters)
- [Crate constants](#crate-constants)
- [Cargo features](#cargo-features)
- [Testing, fuzzing, and benchmarks](#testing-fuzzing-and-benchmarks)
- [Cookbook](#cookbook)
- [End-to-end example](#end-to-end-example)

---

## Overview

Encoders accept any `&mut WriteBuf<'_>` and never allocate. Decoders accept any
`&mut ReadBuf<'de>` and return values that borrow from the input where
possible. Framing strategies are stateless; you call them once per frame and
advance your own input cursor by [`Frame::consumed`](#frame). For streaming
input, [`StreamDecoder`](#streamdecoder) keeps the small amount of state that
makes delimiter search linear.

A typical pipeline is:

1. Build a `Framer` (length-prefixed, varint-prefixed, or delimited,
   optionally wrapped in `Checksummed`).
2. Encode your payload type via `Encode::encode` into a `WriteBuf`.
3. Call `framer.write_frame(payload, &mut wire_buf)` to wrap it.
4. On the read side, call `framer.next_frame(input)` (or
   `decoder.decode(input)`) to extract the payload, then
   `T::decode(&mut ReadBuf::new(frame.payload()))` to deserialize.

**Atomic operations.** Every fallible call in the crate leaves its cursor
unchanged on error, and writes nothing. A streaming reader that gets
`Error::UnexpectedEof` can append bytes and retry the same call.

**No panics on input.** Malformed or hostile bytes never cause a panic.
Lengths read from the wire are converted with checked arithmetic, including
on 32-bit targets. The one assertion in the crate is a `debug_assert!` in
`Frame::new` for custom framers that break the `Frame` contract.

---

## Error handling

### `Error`

```rust,ignore
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
    EmptyDelimiter,
    DelimiterInPayload,
    ChecksumMismatch,
}
```

| Variant | Meaning | Typical response |
| ------- | ------- | ---------------- |
| `UnexpectedEof` | Reader ran out of bytes before the value completed. | Wait for more input and retry; or the message is truncated. |
| `BufferFull` | Writer ran out of capacity. Nothing was written. | Flush or grow the output, then retry. |
| `VarintOverflow` | A varint ran past its maximum length or its value does not fit the target type. | Hostile or corrupt input. |
| `FrameTooLarge { len, limit }` | A frame's payload length exceeds the framer's limit. For delimited framing `len` is a lower bound. Both fields saturate at `usize::MAX`. | Hostile input on decode; oversized payload on encode. |
| `InvalidLengthPrefix` | A length prefix adjusts to a negative length, or a payload cannot be represented under the configured length adjustment. | Hostile or corrupt input. |
| `DelimiterNotFound` | Not produced by the built-in framers (they report an over-long undelimited input as `FrameTooLarge`). | Available to custom framers. |
| `InvalidEncoding` | Bytes violate a structural rule: overlong varint in canonical mode, malformed COBS block, `bool` byte other than `0` / `1`, non-zero padding in a packed struct, or a custom framer breaking the `Frame` contract. | Hostile or corrupt input. |
| `BitOverflow` | A bit-level call asked for zero bits, more than [`MAX_BIT_WIDTH`](#max_bit_width), or a value that does not fit its width. | Fix the call site. |
| `EmptyDelimiter` | A delimited-framer constructor was given an empty delimiter. | Fix the call site. |
| `DelimiterInPayload` | `Delimited::write_frame` was given a payload that the receiver would not read back as one frame. Nothing was written. | Escape the payload (e.g. with [COBS](#cobs)) or use a length prefix. |
| `ChecksumMismatch` | A `Checksummed` frame's trailer does not match its contents. | Discard the frame. |

`Error` implements `core::fmt::Display` and `core::error::Error`
unconditionally, including on `no_std` targets. With the `std` feature it
also converts into `std::io::Error` (kind `UnexpectedEof`, `WriteZero`,
`InvalidInput`, or `InvalidData`, with the original error as the source).

```rust
use wire_codec::{Error, ReadBuf};

let mut buf = ReadBuf::new(&[0x01]);
let result = buf.read_u16_be();
assert!(matches!(result, Err(Error::UnexpectedEof)));
assert_eq!(buf.position(), 0); // atomic: the cursor did not move

# #[cfg(feature = "std")]
# {
let io = std::io::Error::from(Error::ChecksumMismatch);
assert_eq!(io.kind(), std::io::ErrorKind::InvalidData);
# }
```

### `Result<T>`

```rust,ignore
pub type Result<T> = core::result::Result<T, Error>;
```

Used as the return type of every fallible function in the crate.

---

## Buffer cursors

### `ReadBuf`

```rust,ignore
#[derive(Debug, Clone)]
pub struct ReadBuf<'a> { /* ... */ }
```

A cursor over an immutable byte slice with a tracked read position. Reads
borrow from the underlying slice, so they never copy (except the explicit
`read_array`).

| Method | Description |
| ------ | ----------- |
| `pub const fn new(bytes: &'a [u8]) -> Self` | Wrap `bytes` and position at offset zero. |
| `pub const fn position(&self) -> usize` | Current read offset. After an error, the offset of the value that failed. |
| `pub const fn remaining(&self) -> usize` | Bytes left to read. |
| `pub const fn is_empty(&self) -> bool` | `true` if no bytes remain. |
| `pub fn remaining_slice(&self) -> &'a [u8]` | Borrow the unconsumed tail. |
| `pub fn peek(&self, n: usize) -> Result<&'a [u8]>` | Look at the next `n` bytes without advancing. |
| `pub fn peek_u8(&self) -> Result<u8>` | Look at the next byte without advancing. |
| `pub fn read_bytes(&mut self, n: usize) -> Result<&'a [u8]>` | Read `n` bytes and advance. |
| `pub fn read_array<const N: usize>(&mut self) -> Result<[u8; N]>` | Read `N` bytes as an array (copied). |
| `pub fn read_array_ref<const N: usize>(&mut self) -> Result<&'a [u8; N]>` | Borrow `N` bytes as an array reference (zero-copy). |
| `pub fn split_to(&mut self, n: usize) -> Result<ReadBuf<'a>>` | Split the next `n` bytes off as their own cursor. |
| `pub fn advance(&mut self, n: usize) -> Result<()>` | Skip `n` bytes. |
| `pub fn read_u8` / `read_i8` | Read one byte. |
| `pub fn read_{u16,u32,u64,u128}_{be,le}` | Read an unsigned integer in big or little endian. |
| `pub fn read_{i16,i32,i64,i128}_{be,le}` | Read a two's-complement signed integer. |
| `pub fn read_{f32,f64}_{be,le}` | Read an IEEE 754 float. |

Every read returns `Error::UnexpectedEof` when too few bytes remain, and
leaves the position unchanged in that case.

```rust
use wire_codec::ReadBuf;

let mut buf = ReadBuf::new(&[0xDE, 0xAD, 0x02, 0xBE, 0xEF, 0xFF]);
assert_eq!(buf.read_u16_be().unwrap(), 0xDEAD);

// A length-delimited sub-record.
let len = usize::from(buf.read_u8().unwrap());
let mut record = buf.split_to(len).unwrap();
assert_eq!(record.read_u16_le().unwrap(), 0xEFBE);
assert!(record.is_empty());

assert_eq!(buf.peek_u8().unwrap(), 0xFF);
assert_eq!(buf.read_i8().unwrap(), -1);
assert!(buf.is_empty());
```

### `WriteBuf`

```rust,ignore
#[derive(Debug)]
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
| `pub fn write_u8` / `write_i8` | Write one byte. |
| `pub fn write_{u16,u32,u64,u128}_{be,le}` | Write an unsigned integer. |
| `pub fn write_{i16,i32,i64,i128}_{be,le}` | Write a signed integer. |
| `pub fn write_{f32,f64}_{be,le}` | Write an IEEE 754 float. |
| `pub fn into_written(self) -> &'a mut [u8]` | Consume the cursor; borrow the written prefix back. |

Every write returns `Error::BufferFull` when capacity is short, and writes
nothing in that case.

While a `WriteBuf` is alive it mutably borrows its storage, so read the
bytes back through `written()` rather than indexing the storage array.

```rust
use wire_codec::WriteBuf;

let mut storage = [0u8; 8];
let mut buf = WriteBuf::new(&mut storage);
buf.write_u32_be(0xCAFEBABE).unwrap();
buf.write_f32_le(1.0).unwrap();
assert_eq!(buf.written(), &[0xCA, 0xFE, 0xBA, 0xBE, 0x00, 0x00, 0x80, 0x3F]);
assert_eq!(buf.remaining(), 0);
```

---

## Codec traits

### `Encode`

```rust,ignore
pub trait Encode {
    fn encoded_size(&self) -> usize;
    fn encode(&self, buf: &mut WriteBuf<'_>) -> Result<()>;
}
```

`encoded_size` must report the exact number of bytes `encode` will write on
success. Pairing the two lets callers size an output buffer up front.

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

let mut out = [0u8; 5];
Tagged { tag: 1, value: 2 }.encode(&mut WriteBuf::new(&mut out)).unwrap();
assert_eq!(out, [1, 0, 0, 0, 2]);
```

### `Decode`

```rust,ignore
pub trait Decode<'de>: Sized {
    fn decode(buf: &mut ReadBuf<'de>) -> Result<Self>;
}
```

The lifetime `'de` ties the decoded value to the input slice, so types that
borrow (`&'de [u8]`, `&'de [u8; N]`, `&'de str`) can be returned without
copying.

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

let tagged = Tagged::decode(&mut ReadBuf::new(&[1, 0, 0, 0, 2])).unwrap();
assert_eq!((tagged.tag, tagged.value), (1, 2));
```

### Provided implementations

| Type | `Encode` | `Decode` | Wire form |
| ---- | -------- | -------- | --------- |
| `u8`, `i8` | yes | yes | one byte |
| `bool` | yes | yes | one byte, `0` or `1`; anything else is `InvalidEncoding` |
| `[u8; N]` | yes | yes | `N` raw bytes (copied on decode) |
| `&'de [u8; N]` | no | yes | `N` raw bytes (zero-copy) |
| `[u8]` | yes | no | raw bytes; frame the length separately |
| `Be<T>`, `Le<T>` | yes | yes | `T` in big / little endian, for `u16`..`u128`, `i16`..`i128`, `f32`, `f64` |
| `Varint<T>` | yes | yes | LEB128 for `u16`, `u32`, `u64`, `usize`; zigzag LEB128 for `i32`, `i64` |
| tuples of 2 to 6 elements | yes | yes | elements in order; atomic as a whole |

Plain multi-byte integers have no `Encode` impl on purpose: the byte order is
part of the wire format and must be chosen explicitly.

### `Be` / `Le` / `Varint`

```rust,ignore
pub struct Be<T>(pub T);
pub struct Le<T>(pub T);
pub struct Varint<T>(pub T);
```

Transparent newtypes that select a wire representation. All three derive
`Debug`, `Clone`, `Copy`, `Default`, `PartialEq`, `Eq`, `PartialOrd`, `Ord`,
and `Hash` (where `T` does).

```rust
use wire_codec::{Be, Decode, Encode, Le, ReadBuf, Varint, WriteBuf};

let record = (Be(0x0102u16), Le(0x0102u16), Varint(-1i32));
let mut out = [0u8; 8];
let mut buf = WriteBuf::new(&mut out);
record.encode(&mut buf).unwrap();
assert_eq!(buf.written(), &[0x01, 0x02, 0x02, 0x01, 0x01]);
assert_eq!(record.encoded_size(), 5);

let back = <(Be<u16>, Le<u16>, Varint<i32>)>::decode(&mut ReadBuf::new(&out[..5])).unwrap();
assert_eq!(back, record);
```

---

## Varint encoding

LEB128 varint. The low seven bits of each byte carry payload; the top bit
signals continuation. Compatible with Protocol Buffers, WebAssembly, and
DWARF. The signed functions apply [zigzag](#zigzag-encoding) first
(protobuf `sint32` / `sint64`).

Every function in the module is atomic: on error the cursor is unchanged.

### Varint constants

```rust,ignore
pub const MAX_LEN_U16: usize = 3;
pub const MAX_LEN_U32: usize = 5;
pub const MAX_LEN_U64: usize = 10;
```

Maximum encoded byte length for the corresponding target width (`usize`,
`i64` use `MAX_LEN_U64`; `i32` uses `MAX_LEN_U32`).

### Encoded length

```rust,ignore
pub const fn encoded_len_u16(value: u16) -> usize;   // 1..=3
pub const fn encoded_len_u32(value: u32) -> usize;   // 1..=5
pub const fn encoded_len_u64(value: u64) -> usize;   // 1..=10
pub const fn encoded_len_usize(value: usize) -> usize;
pub const fn encoded_len_i32(value: i32) -> usize;   // zigzag
pub const fn encoded_len_i64(value: i64) -> usize;   // zigzag
```

```rust
use wire_codec::varint;
assert_eq!(varint::encoded_len_u32(127), 1);
assert_eq!(varint::encoded_len_u32(128), 2);
assert_eq!(varint::encoded_len_u64(u64::MAX), 10);
assert_eq!(varint::encoded_len_i64(-1), 1);
```

### Encoding

```rust,ignore
pub fn encode_u16(value: u16, buf: &mut WriteBuf<'_>) -> Result<()>;
pub fn encode_u32(value: u32, buf: &mut WriteBuf<'_>) -> Result<()>;
pub fn encode_u64(value: u64, buf: &mut WriteBuf<'_>) -> Result<()>;
pub fn encode_usize(value: usize, buf: &mut WriteBuf<'_>) -> Result<()>;
pub fn encode_i32(value: i32, buf: &mut WriteBuf<'_>) -> Result<()>;
pub fn encode_i64(value: i64, buf: &mut WriteBuf<'_>) -> Result<()>;
```

Returns `Error::BufferFull` (writing nothing) when the cursor lacks room for
the encoded length. Encoders always produce the canonical (shortest) form.

```rust
use wire_codec::{varint, WriteBuf};

let mut out = [0u8; 10];
let mut buf = WriteBuf::new(&mut out);
varint::encode_u32(300, &mut buf).unwrap();
varint::encode_i64(-2, &mut buf).unwrap();
assert_eq!(buf.written(), &[0xAC, 0x02, 0x03]);
```

### Decoding

```rust,ignore
pub fn decode_u16(buf: &mut ReadBuf<'_>) -> Result<u16>;
pub fn decode_u32(buf: &mut ReadBuf<'_>) -> Result<u32>;
pub fn decode_u64(buf: &mut ReadBuf<'_>) -> Result<u64>;
pub fn decode_usize(buf: &mut ReadBuf<'_>) -> Result<usize>;
pub fn decode_i32(buf: &mut ReadBuf<'_>) -> Result<i32>;
pub fn decode_i64(buf: &mut ReadBuf<'_>) -> Result<i64>;
```

- `Error::UnexpectedEof` if the input ends mid-varint.
- `Error::VarintOverflow` if the varint runs past its maximum length, or the
  value does not fit the target (for `decode_usize`, values above
  `usize::MAX` on 32-bit hosts).

These decoders accept overlong encodings (redundant `0x80` bytes before a
`0x00` terminator), as protobuf parsers do.

```rust
use wire_codec::{varint, Error, ReadBuf};

let mut buf = ReadBuf::new(&[0xAC, 0x02]);
assert_eq!(varint::decode_u32(&mut buf).unwrap(), 300);

// Truncated: error, cursor unchanged, retry once more bytes arrive.
let mut buf = ReadBuf::new(&[0xAC]);
assert_eq!(varint::decode_u32(&mut buf), Err(Error::UnexpectedEof));
assert_eq!(buf.position(), 0);
```

### Canonical decoding

```rust,ignore
pub fn decode_u16_canonical(buf: &mut ReadBuf<'_>) -> Result<u16>;
pub fn decode_u32_canonical(buf: &mut ReadBuf<'_>) -> Result<u32>;
pub fn decode_u64_canonical(buf: &mut ReadBuf<'_>) -> Result<u64>;
pub fn decode_usize_canonical(buf: &mut ReadBuf<'_>) -> Result<usize>;
pub fn decode_i32_canonical(buf: &mut ReadBuf<'_>) -> Result<i32>;
pub fn decode_i64_canonical(buf: &mut ReadBuf<'_>) -> Result<i64>;
```

Same as the plain decoders, plus `Error::InvalidEncoding` for overlong
encodings, so every value has exactly one accepted wire form. Use these when
the bytes are hashed, signed, deduplicated, or compared across
implementations.

```rust
use wire_codec::{varint, Error, ReadBuf};

let overlong = [0x80, 0x00]; // zero, padded to two bytes
assert_eq!(varint::decode_u32(&mut ReadBuf::new(&overlong)), Ok(0));
assert_eq!(
    varint::decode_u32_canonical(&mut ReadBuf::new(&overlong)),
    Err(Error::InvalidEncoding)
);
```

---

## Zigzag encoding

Maps signed integers to unsigned ones so that small-magnitude negative values
produce small-magnitude varints.

| signed | unsigned |
| ------ | -------- |
|  0 | 0 |
| -1 | 1 |
|  1 | 2 |
| -2 | 3 |
|  2 | 4 |

```rust,ignore
pub const fn encode_i32(value: i32) -> u32;
pub const fn decode_i32(value: u32) -> i32;
pub const fn encode_i64(value: i64) -> u64;
pub const fn decode_i64(value: u64) -> i64;
```

```rust
use wire_codec::zigzag;
assert_eq!(zigzag::encode_i32(-1), 1);
assert_eq!(zigzag::decode_i32(1), -1);
assert_eq!(zigzag::encode_i32(i32::MIN), u32::MAX);
assert_eq!(zigzag::encode_i64(i64::MIN), u64::MAX);
assert_eq!(zigzag::decode_i64(u64::MAX), i64::MIN);
```

---

## Bitfield cursors

Bit-level read and write cursors with most-significant-bit-first ordering.
Useful for packed protocols where multiple fields share a single byte.

### `BitReader`

```rust,ignore
#[derive(Debug, Clone)]
pub struct BitReader<'a> { /* ... */ }
```

| Method | Description |
| ------ | ----------- |
| `pub const fn new(bytes: &'a [u8]) -> Self` | Wrap a byte slice. |
| `pub const fn bits_consumed(&self) -> usize` | Bits read so far. |
| `pub const fn bits_remaining(&self) -> usize` | Bits still available. |
| `pub fn align_to_byte(&mut self)` | Skip to the next byte boundary (no-op if aligned). |
| `pub fn read_bits(&mut self, n: u32) -> Result<u64>` | Read `1..=64` bits into the low end of a `u64`. |
| `pub fn read_bool(&mut self) -> Result<bool>` | Read one bit. |
| `pub fn read_signed_bits(&mut self, n: u32) -> Result<i64>` | Read `n` bits as two's complement, sign-extended. |

Errors: `BitOverflow` for a width of `0` or above 64, `UnexpectedEof` when
too few bits remain. The cursor does not move on error.

```rust
use wire_codec::BitReader;

let mut r = BitReader::new(&[0xAC, 0b1110_0000]); // 0b1010_1100
assert_eq!(r.read_bits(3).unwrap(), 0b101);
assert_eq!(r.read_bits(5).unwrap(), 0b01100);
assert!(r.read_bool().unwrap());
assert_eq!(r.read_signed_bits(2).unwrap(), -1);
r.align_to_byte();
assert_eq!(r.bits_remaining(), 0);
```

### `BitWriter`

```rust,ignore
#[derive(Debug)]
pub struct BitWriter<'a> { /* ... */ }
```

| Method | Description |
| ------ | ----------- |
| `pub fn new(bytes: &'a mut [u8]) -> Self` | Wrap a mutable byte slice. |
| `pub fn bits_written(&self) -> usize` | Bits written so far. |
| `pub fn align_to_byte(&mut self) -> Result<()>` | Zero-pad to the next byte boundary. Never fails in practice. |
| `pub fn write_bits(&mut self, value: u64, n: u32) -> Result<()>` | Write the low `n` bits of `value`. |
| `pub fn write_bool(&mut self, value: bool) -> Result<()>` | Write one bit. |
| `pub fn write_signed_bits(&mut self, value: i64, n: u32) -> Result<()>` | Write `value` as an `n`-bit two's-complement field. |
| `pub fn finish(self) -> usize` | Zero the trailing bits of a partial byte; return the whole-byte count. |

Every bit inside the finished prefix is written explicitly (field bits, or
zero padding), so a reused buffer with stale contents produces the same
output as a zeroed one. Errors: `BitOverflow` for a bad width or a value
that does not fit, `BufferFull` when the buffer is out of bits.

```rust
use wire_codec::BitWriter;

let mut storage = [0xFFu8; 2]; // stale contents are overwritten
let mut w = BitWriter::new(&mut storage);
w.write_bits(0b101, 3).unwrap();
w.align_to_byte().unwrap();
w.write_signed_bits(-1, 2).unwrap();
assert_eq!(w.finish(), 2);
assert_eq!(storage, [0b1010_0000, 0b1100_0000]);
```

### `MAX_BIT_WIDTH`

```rust,ignore
pub const MAX_BIT_WIDTH: u32 = 64;
```

Upper bound on `n` for every bit-level read and write.

---

## Packed structs

### `packed_struct!`

```rust,ignore
packed_struct! {
    $(#[$meta])* $vis struct $Name {
        $( $(#[$field_meta])* $field_vis $field: $Type = $bits, )+
    }
}
```

Declares the struct as written (attributes and visibility pass through) and
generates:

- `const BITS: u32` (sum of the widths) and `const SIZE: usize` (bytes).
- `fn pack(&self) -> Result<[u8; SIZE]>` and
  `fn unpack(&[u8; SIZE]) -> Result<Self>`.
- `Encode` (writes `SIZE` bytes) and `Decode` (reads `SIZE` bytes,
  atomically).

Rules: widths are checked at compile time (zero, or wider than the field
type, is a compile error); out-of-range values fail with `BitOverflow`;
padding bits in the last byte are written as zero and must be zero on
decode (`InvalidEncoding` otherwise).

```rust
use wire_codec::{packed_struct, Decode, Encode, ReadBuf, WriteBuf};

packed_struct! {
    /// The first 32 bits of an IPv4 header.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Ipv4Head {
        pub version: u8 = 4,
        pub ihl: u8 = 4,
        pub dscp: u8 = 6,
        pub ecn: u8 = 2,
        pub total_length: u16 = 16,
    }
}

let head = Ipv4Head { version: 4, ihl: 5, dscp: 0, ecn: 0, total_length: 84 };
assert_eq!(head.pack().unwrap(), [0x45, 0x00, 0x00, 0x54]);

let mut out = [0u8; Ipv4Head::SIZE];
head.encode(&mut WriteBuf::new(&mut out)).unwrap();
assert_eq!(Ipv4Head::decode(&mut ReadBuf::new(&out)).unwrap(), head);
```

### `PackedField`

```rust,ignore
pub trait PackedField: Sized + Copy {
    const MAX_BITS: u32;
    fn to_bits(self, bits: u32) -> Result<u64>;
    fn from_bits(raw: u64, bits: u32) -> Result<Self>;
}
```

Implemented for `u8`..`u64`, `i8`..`i64` (two's complement in the declared
width), and `bool` (one bit). Implement it on your own enums to pack them
directly:

```rust
use wire_codec::packed::PackedField;
use wire_codec::{Error, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Opcode { Read, Write }

impl PackedField for Opcode {
    const MAX_BITS: u32 = 1;
    fn to_bits(self, _bits: u32) -> Result<u64> { Ok(self as u64) }
    fn from_bits(raw: u64, _bits: u32) -> Result<Self> {
        match raw {
            0 => Ok(Opcode::Read),
            1 => Ok(Opcode::Write),
            _ => Err(Error::InvalidEncoding),
        }
    }
}

assert_eq!(Opcode::from_bits(1, 1), Ok(Opcode::Write));
```

---

## Framing

A frame is a delimited unit of bytes on the wire. The `Framer` trait defines
a stateless extraction strategy. All framers are re-exported from
`wire_codec::framing` (their defining submodules `framing::length`,
`framing::varint_prefixed`, `framing::delimiter`, and
`framing::checksummed` are public too).

### `Frame`

```rust,ignore
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Frame<'a> { /* ... */ }

impl<'a> Frame<'a> {
    pub const fn new(payload: &'a [u8], consumed: usize) -> Self;
    pub const fn payload(&self) -> &'a [u8];
    pub const fn consumed(&self) -> usize;
}
```

Carries the zero-copy payload borrow alongside the byte count the framer
consumed (header, delimiter, and trailer included). Advance your input cursor
by `consumed` after each call. Custom framers must return
`consumed >= payload.len()` (checked by a `debug_assert!`) and
`consumed > 0`.

```rust
use wire_codec::framing::Frame;

let frame = Frame::new(b"abc", 4);
assert_eq!((frame.payload(), frame.consumed()), (&b"abc"[..], 4));
```

### `Framer`

```rust,ignore
pub trait Framer {
    fn next_frame<'a>(&self, input: &'a [u8]) -> Result<Option<Frame<'a>>>;
    fn write_frame(&self, payload: &[u8], out: &mut WriteBuf<'_>) -> Result<()>;

    // Provided:
    fn next_frame_from<'a>(&self, input: &'a [u8], scanned: &mut usize)
        -> Result<Option<Frame<'a>>>;
    fn max_frame_len(&self, payload_len: usize) -> Option<usize>;
}
```

`next_frame` returns `Ok(Some(frame))` when a complete frame is available,
`Ok(None)` when more bytes are needed, and `Err(_)` when the input violates the
protocol.

`next_frame_from` is the resumable form for streaming input. `scanned` is a
progress hint owned by the caller, starting at `0`; between calls only
append to `input`, and after a frame drop its `consumed()` bytes. Searching
framers (`Delimited`) resume from the hint; others ignore it. The default
implementation calls `next_frame` and resets the hint after a frame.

`max_frame_len` is an upper bound on the bytes `write_frame` writes for a
payload of the given length (`None` if unknown); buffer-growing adapters use
it to reserve space in one step. The built-in framers all implement it and
all make `write_frame` atomic.

```rust
use wire_codec::framing::{Framer, LengthPrefixed, LengthWidth, Endian};

let framer = LengthPrefixed::new(LengthWidth::U16, Endian::Big);
let frame = framer.next_frame(&[0x00, 0x03, b'h', b'i', b'!']).unwrap().unwrap();
assert_eq!(frame.payload(), b"hi!");
assert_eq!(framer.max_frame_len(3), Some(5));
```

### `StreamDecoder`

```rust,ignore
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct StreamDecoder<F> { /* ... */ }

impl<F: Framer> StreamDecoder<F> {
    pub const fn new(framer: F) -> Self;
    pub fn decode<'a>(&mut self, input: &'a [u8]) -> Result<Option<Frame<'a>>>;
    pub fn reset(&mut self);
    pub const fn framer(&self) -> &F;
    pub fn into_inner(self) -> F;
}
```

Owns the `next_frame_from` hint so a growing buffer is searched in linear
time overall.

```rust
use wire_codec::framing::{Delimited, StreamDecoder};

let mut decoder = StreamDecoder::new(Delimited::bounded(b"\n").unwrap());
assert_eq!(decoder.decode(b"par").unwrap(), None);
assert_eq!(decoder.decode(b"partial\n").unwrap().unwrap().payload(), b"partial");
```

### `LengthWidth`

```rust,ignore
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LengthWidth { U8, U16, U32, U24, U64 }

impl LengthWidth {
    pub const fn header_size(self) -> usize; // 1, 2, 4, 3, 8
    pub const fn max_payload(self) -> u64;   // largest encodable value
}
```

`U24` (HTTP/2, TLS handshake, MySQL) and `U64` were added in 1.1. On 32-bit
targets a `U64` length that does not fit in `usize` is reported as
`FrameTooLarge`.

```rust
use wire_codec::framing::LengthWidth;
assert_eq!(LengthWidth::U24.header_size(), 3);
assert_eq!(LengthWidth::U24.max_payload(), 16_777_215);
```

### `Endian`

```rust,ignore
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Endian { Big, Little }
```

Byte order for a length prefix or checksum trailer.

### `LengthPrefixed`

```rust,ignore
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LengthPrefixed { /* ... */ }

impl LengthPrefixed {
    pub const DEFAULT_MAX_PAYLOAD: u64 = 8 * 1024 * 1024;
    pub const fn new(width: LengthWidth, endian: Endian) -> Self;
    pub const fn bounded(width: LengthWidth, endian: Endian) -> Self;
    pub const fn with_max_payload(self, max: u64) -> Self;
    pub const fn with_length_adjustment(self, adjustment: i64) -> Self;
    pub const fn width(&self) -> LengthWidth;
    pub const fn endian(&self) -> Endian;
    pub const fn max_payload(&self) -> u64;
    pub const fn length_adjustment(&self) -> i64;
}
```

Each frame is an unsigned integer giving the payload length, then the
payload. `new` allows the width's full range (4 GiB for `U32`); `bounded`
caps at `DEFAULT_MAX_PAYLOAD`; `with_max_payload` clamps to the width's
maximum. The length adjustment says "payload length = prefix value +
adjustment" (use `-(header_size)` when the prefix counts itself).

Errors: `FrameTooLarge` above the limit, `InvalidLengthPrefix` when the
adjustment yields a negative length, `BufferFull` on a short output.

```rust
use wire_codec::WriteBuf;
use wire_codec::framing::{Endian, Framer, LengthPrefixed, LengthWidth};

let framer = LengthPrefixed::bounded(LengthWidth::U16, Endian::Big);
let mut out = [0u8; 16];
let mut buf = WriteBuf::new(&mut out);
framer.write_frame(b"hello", &mut buf).unwrap();
assert_eq!(buf.written(), &[0x00, 0x05, b'h', b'e', b'l', b'l', b'o']);

let frame = framer.next_frame(buf.written()).unwrap().unwrap();
assert_eq!(frame.payload(), b"hello");
assert_eq!(frame.consumed(), 7);

// A self-counting 4-byte prefix.
let framer = LengthPrefixed::new(LengthWidth::U32, Endian::Big).with_length_adjustment(-4);
let frame = framer.next_frame(&[0, 0, 0, 6, b'o', b'k']).unwrap().unwrap();
assert_eq!(frame.payload(), b"ok");
```

### `VarintPrefixed`

```rust,ignore
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VarintPrefixed { /* ... */ }

impl VarintPrefixed {
    pub const DEFAULT_MAX_PAYLOAD: usize = 8 * 1024 * 1024;
    pub const fn new() -> Self;                       // bounded, canonical
    pub const fn with_max_payload(self, max: usize) -> Self;
    pub const fn with_canonical(self, canonical: bool) -> Self;
    pub const fn max_payload(&self) -> usize;
    pub const fn is_canonical(&self) -> bool;
}
impl Default for VarintPrefixed { /* new() */ }
```

LEB128 length, then the payload (the protobuf length-delimited stream
format). Errors: `VarintOverflow` for a prefix longer than ten bytes,
`InvalidEncoding` for an overlong prefix in canonical mode, `FrameTooLarge`
above the limit, `BufferFull` on a short output.

```rust
use wire_codec::WriteBuf;
use wire_codec::framing::{Framer, VarintPrefixed};

let framer = VarintPrefixed::new();
let mut out = [0u8; 300];
let mut buf = WriteBuf::new(&mut out);
framer.write_frame(&[7u8; 200], &mut buf).unwrap();
assert_eq!(&buf.written()[..2], &[0xC8, 0x01]); // 200 as a varint
let frame = framer.next_frame(buf.written()).unwrap().unwrap();
assert_eq!(frame.payload().len(), 200);
```

### `Delimited`

```rust,ignore
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Delimited<'d> { /* ... */ }

impl<'d> Delimited<'d> {
    pub const DEFAULT_MAX_PAYLOAD: usize = 64 * 1024;
    pub const fn new(delimiter: &'d [u8]) -> Result<Self>;      // unbounded
    pub const fn bounded(delimiter: &'d [u8]) -> Result<Self>;  // 64 KiB
    pub const fn with_max_payload(self, max: usize) -> Self;
    pub const fn delimiter(&self) -> &'d [u8];
    pub const fn max_payload(&self) -> usize;
}
```

Frames are separated by a byte sequence (one or more bytes). The delimiter is
stripped from the emitted payload but counted toward `Frame::consumed`. An
empty delimiter yields `Error::EmptyDelimiter`.

- **Search.** A portable SWAR scan for the delimiter's first byte (16 bytes
  per step, safe code), then a tail compare; with the `memchr` feature, the
  `memchr` crate's SIMD search. The search never looks further than
  `max_payload + delimiter.len()` bytes.
- **Limits.** Once that many bytes are buffered without a delimiter, the
  framer returns `FrameTooLarge` (with `len = max_payload + 1`, a lower
  bound).
- **Streaming.** `next_frame_from` / `StreamDecoder` resume the search,
  keeping `delimiter.len() - 1` bytes of overlap for a split delimiter.
- **Writing.** `write_frame` returns `DelimiterInPayload` (and writes
  nothing) for a payload the receiver would not read back as one frame:
  one that contains the delimiter, or one whose tail merges with a
  self-overlapping delimiter (`b"a"` before `b"aa"`).

```rust
use wire_codec::framing::{Delimited, Framer};
use wire_codec::{Error, WriteBuf};

let framer = Delimited::bounded(b"\r\n").unwrap();
let frame = framer.next_frame(b"GET /\r\nrest").unwrap().unwrap();
assert_eq!(frame.payload(), b"GET /");
assert_eq!(frame.consumed(), 7);

let mut out = [0u8; 16];
let mut buf = WriteBuf::new(&mut out);
assert_eq!(framer.write_frame(b"a\r\nb", &mut buf), Err(Error::DelimiterInPayload));
```

### `Checksummed` / `ChecksumScope`

```rust,ignore
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Checksummed<F, C> { /* ... */ }

impl<F: Framer, C: Checksum> Checksummed<F, C> {
    pub const fn new(inner: F, checksum: C, endian: Endian) -> Self;
    pub const fn with_scope(self, scope: ChecksumScope) -> Self;
    pub const fn inner(&self) -> &F;
    pub const fn checksum(&self) -> &C;
    pub const fn endian(&self) -> Endian;
    pub const fn scope(&self) -> ChecksumScope;
}

#[non_exhaustive]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum ChecksumScope { #[default] Frame, Payload }
```

Appends `C::LEN` checksum bytes after each frame of the inner framer, and
verifies them on decode (`ChecksumMismatch` on failure). By default the
checksum covers the whole inner frame (header, payload, delimiter);
`ChecksumScope::Payload` covers the payload only. Streaming hints are
forwarded to the inner framer.

```rust
use wire_codec::checksum::Crc16CcittFalse;
use wire_codec::framing::{Checksummed, ChecksumScope, Endian, Framer, LengthPrefixed, LengthWidth};
use wire_codec::WriteBuf;

let framer = Checksummed::new(
    LengthPrefixed::new(LengthWidth::U8, Endian::Big),
    Crc16CcittFalse,
    Endian::Big,
)
.with_scope(ChecksumScope::Payload);

let mut out = [0u8; 16];
let mut buf = WriteBuf::new(&mut out);
framer.write_frame(b"123456789", &mut buf).unwrap();
assert_eq!(&buf.written()[10..], &[0x29, 0xB1]); // CRC-16/CCITT-FALSE check value
assert_eq!(framer.next_frame(buf.written()).unwrap().unwrap().payload(), b"123456789");
```

---

## Checksums

```rust,ignore
pub trait Checksum {
    const LEN: usize;                       // 1..=8 bytes on the wire
    fn compute(&self, data: &[u8]) -> u64;  // low LEN * 8 bits are sent
}

pub struct Crc16CcittFalse;  // CRC-16/IBM-3740: poly 0x1021, init 0xFFFF
pub struct Crc32;            // CRC-32/ISO-HDLC (IEEE 802.3, zlib)
pub struct Crc32c;           // CRC-32/ISCSI (Castagnoli)

pub fn crc16_ccitt_false(data: &[u8]) -> u16;
pub fn crc32(data: &[u8]) -> u32;
pub fn crc32c(data: &[u8]) -> u32;
```

Table-driven, `const`-generated lookup tables, no dependencies. A CRC
detects accidental corruption only; it does not authenticate the sender.

```rust
use wire_codec::checksum::{self, Checksum, Crc32c};

assert_eq!(checksum::crc16_ccitt_false(b"123456789"), 0x29B1);
assert_eq!(checksum::crc32(b"123456789"), 0xCBF4_3926);
assert_eq!(Crc32c.compute(b"123456789"), 0xE306_9283);
```

---

## COBS

```rust,ignore
pub const fn max_encoded_len(len: usize) -> usize;
pub fn encode(src: &[u8], out: &mut WriteBuf<'_>) -> Result<()>;
pub fn decode(src: &[u8], out: &mut WriteBuf<'_>) -> Result<()>;
pub fn decode_in_place(buf: &mut [u8]) -> Result<usize>;
```

Consistent Overhead Byte Stuffing removes every `0x00` from a payload so a
zero byte can delimit frames. Pair it with `Delimited` on `[0x00]`.
`encode` never writes more than `max_encoded_len` bytes and never emits a
trailing empty block. Decoding rejects empty input, embedded zeros, and
code bytes that run past the block with `InvalidEncoding`. Encode and decode
into a `WriteBuf` are atomic.

```rust
use wire_codec::{cobs, WriteBuf};

let mut wire = [0u8; 8];
let mut out = WriteBuf::new(&mut wire);
cobs::encode(&[0x11, 0x00, 0x22], &mut out).unwrap();
assert_eq!(out.written(), &[0x02, 0x11, 0x02, 0x22]);

let mut block = [0x02, 0x11, 0x02, 0x22];
let len = cobs::decode_in_place(&mut block).unwrap();
assert_eq!(&block[..len], &[0x11, 0x00, 0x22]);
```

---

## `bytes` / Tokio / futures adapters

Available with the `bytes` feature (`codec` module):

```rust,ignore
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct FrameCodec<F> { /* ... */ }

impl<F: Framer> FrameCodec<F> {
    pub const fn new(framer: F) -> Self;
    pub const fn framer(&self) -> &F;
    pub fn into_inner(self) -> F;
    pub fn decode_frame(&mut self, src: &mut BytesMut) -> Result<Option<BytesMut>>;
    pub fn decode_frame_eof(&mut self, src: &mut BytesMut) -> Result<Option<BytesMut>>;
    pub fn encode_frame(&mut self, payload: &[u8], dst: &mut BytesMut) -> Result<()>;
}
```

`decode_frame` splits the payload off the buffer without copying and keeps
the resumable-scan state. `encode_frame` reserves `max_frame_len` bytes in
one step. Both leave the buffer untouched on error.

With `tokio-util`: `tokio_util::codec::Decoder` (`Item = BytesMut`) and
`Encoder<&[u8]>`, `Encoder<Bytes>`, `Encoder<BytesMut>`. With
`asynchronous-codec`: `asynchronous_codec::Decoder` and `Encoder`
(`Item<'a> = &'a [u8]`). The trait impls use `std::io::Error`; the original
`Error` is its source.

```rust,ignore
use tokio_util::codec::Framed;
use wire_codec::codec::FrameCodec;
use wire_codec::framing::VarintPrefixed;

let framed = Framed::new(socket, FrameCodec::new(VarintPrefixed::new()));
```

---

## Crate constants

### `VERSION`

```rust,ignore
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
```

Crate version string, populated at build time.

```rust
assert!(!wire_codec::VERSION.is_empty());
```

---

## Cargo features

| Feature | Default | Effect |
| ------- | ------- | ------ |
| `std`   | yes     | Links the standard library; adds `From<Error> for std::io::Error`. Drop it for `no_std`. |
| `memchr` | no     | Delimiter search through the `memchr` crate. `no_std`-compatible. |
| `bytes` | no      | The `codec` module (`FrameCodec` over `bytes::BytesMut`). `no_std` + `alloc`. |
| `tokio-util` | no | `tokio_util::codec` trait impls for `FrameCodec`. Implies `bytes`, `std`. |
| `asynchronous-codec` | no | `asynchronous_codec` trait impls for `FrameCodec`. Implies `bytes`, `std`. |

The crate has no required runtime dependencies.

---

## Testing, fuzzing, and benchmarks

The repository ships with:

- **Unit tests** alongside each module (`src/**/tests`), covering every
  error path and limit boundary.
- **Integration tests** in `tests/`:
  - `smoke.rs` — minimal cross-module pipeline check.
  - `integration_protocols.rs` — realistic protocol shapes.
  - `packed.rs` — `packed_struct!` used from a strictly linted crate.
  - `adapters.rs` — the `tokio-util` and `asynchronous-codec` trait impls.
- **Property tests** (`proptest`) in `tests/prop_*.rs`: varint round-trip,
  atomicity and canonical form, zigzag bijection, framer round-trip across
  every width and endian, streaming-versus-batch equivalence, and bitfield
  round-trip.
- **Fuzzing.** `fuzz/` holds `cargo fuzz` targets for every decoder
  (`varint`, `length_prefixed`, `varint_prefixed`, `delimited`,
  `checksummed`, `bitfield`, `cobs`, `packed`). The invariants live in
  `tests/support/fuzz_harness.rs` and also run under proptest in
  `tests/fuzz_harness.rs`, so `cargo test` exercises them on every platform.
- **Benchmarks** in `benches/` (`codec.rs`, `framing.rs`), with every decode
  input behind `black_box`.

```sh
cargo test --all-features
cargo +nightly fuzz run delimited -- -max_total_time=60
cargo bench --bench codec
cargo bench --bench framing
```

---

## Cookbook

Short recipes for the most common shapes you will hit when building a binary
protocol on top of `wire-codec`.

### Append-only stream over a growing buffer

When you control the input buffer (e.g. a `Vec<u8>` you are draining), the
typical loop is: feed bytes in, read as many complete frames as you can, drop
the consumed prefix, repeat. A `StreamDecoder` keeps delimiter searches
linear.

```rust
use wire_codec::framing::{Endian, LengthPrefixed, LengthWidth, StreamDecoder};

let mut decoder = StreamDecoder::new(LengthPrefixed::bounded(LengthWidth::U16, Endian::Big));
let mut buffer: Vec<u8> = Vec::new();

# fn read_chunk_from_transport() -> Vec<u8> { vec![0x00, 0x05, b'h', b'e', b'l', b'l', b'o'] }
buffer.extend_from_slice(&read_chunk_from_transport());

let mut consumed = 0;
while let Some(frame) = decoder.decode(&buffer[consumed..]).unwrap() {
    // process(frame.payload());
    consumed += frame.consumed();
}
buffer.drain(..consumed);
assert!(buffer.is_empty());
```

### Fixed-output buffer (e.g. a stack-allocated MTU buffer)

When you write into a buffer of known capacity, treat `BufferFull` as a
back-pressure signal: stop encoding, flush downstream, then resume. Because
`write_frame` is atomic, a frame that did not fit leaves no partial bytes
behind.

```rust
use wire_codec::framing::{Endian, Framer, LengthPrefixed, LengthWidth};
use wire_codec::{Error, WriteBuf};

let framer = LengthPrefixed::new(LengthWidth::U16, Endian::Big);
let mut wire = [0u8; 1500]; // one Ethernet MTU
let mut out = WriteBuf::new(&mut wire);

for payload in [b"alpha".as_slice(), b"beta", b"gamma"] {
    match framer.write_frame(payload, &mut out) {
        Ok(()) => {}
        Err(Error::BufferFull) => break, // flush out.written() downstream
        Err(e) => panic!("protocol error: {e}"),
    }
}
assert_eq!(out.position(), 7 + 6 + 7);
```

### Length-prefixed envelope around a structured record

Combine `Encode` / `Decode` for the record shape with a framer for the
on-the-wire envelope.

```rust
use wire_codec::{Decode, Encode, ReadBuf, Result, WriteBuf, varint};

struct Greeting<'a> { id: u32, who: &'a [u8] }

impl<'a> Encode for Greeting<'a> {
    fn encoded_size(&self) -> usize {
        varint::encoded_len_u32(self.id)
            + varint::encoded_len_usize(self.who.len())
            + self.who.len()
    }
    fn encode(&self, buf: &mut WriteBuf<'_>) -> Result<()> {
        varint::encode_u32(self.id, buf)?;
        varint::encode_usize(self.who.len(), buf)?;
        buf.write_bytes(self.who)
    }
}

impl<'de> Decode<'de> for Greeting<'de> {
    fn decode(buf: &mut ReadBuf<'de>) -> Result<Self> {
        let id = varint::decode_u32(buf)?;
        let len = varint::decode_usize(buf)?;
        let who = buf.read_bytes(len)?;
        Ok(Self { id, who })
    }
}

let greeting = Greeting { id: 7, who: b"world" };
let mut out = [0u8; 16];
let mut buf = WriteBuf::new(&mut out);
greeting.encode(&mut buf).unwrap();
assert_eq!(buf.position(), greeting.encoded_size());
let back = Greeting::decode(&mut ReadBuf::new(buf.written())).unwrap();
assert_eq!((back.id, back.who), (7, &b"world"[..]));
```

### Hot-loop pattern: pre-sized output

For high-throughput emit paths, ask the record for its `encoded_size()`,
size a scratch buffer once, and reuse it across iterations. No allocation
on the hot path.

```rust
# use wire_codec::{Encode, Result, WriteBuf};
# struct Record(u8);
# impl Encode for Record {
#   fn encoded_size(&self) -> usize { 1 }
#   fn encode(&self, b: &mut WriteBuf<'_>) -> Result<()> { b.write_u8(self.0) }
# }
# let records = vec![Record(1), Record(2)];
let max_size = records.iter().map(Encode::encoded_size).max().unwrap_or(0);
let mut scratch = vec![0u8; max_size];

for record in &records {
    let mut out = WriteBuf::new(&mut scratch);
    record.encode(&mut out)?;
    let bytes = out.written();
    // emit(bytes);
    # assert_eq!(bytes.len(), 1);
}
# Ok::<(), wire_codec::Error>(())
```

### Serial link with COBS and CRC

Stuff each CRC-protected payload, terminate with `0x00`, and let the
receiver resynchronise at the next zero after line noise.

```rust
use wire_codec::checksum;
use wire_codec::framing::{Delimited, Framer};
use wire_codec::{ReadBuf, WriteBuf, cobs};

// Sender: payload + CRC-16, COBS-stuffed, zero-terminated.
let payload = [0x00, 0x42, 0x00];
let mut packet = [0u8; 8];
let mut p = WriteBuf::new(&mut packet);
p.write_bytes(&payload).unwrap();
p.write_u16_be(checksum::crc16_ccitt_false(&payload)).unwrap();
let packet_len = p.position();

let mut wire = [0u8; 16];
let mut w = WriteBuf::new(&mut wire);
cobs::encode(&packet[..packet_len], &mut w).unwrap();
w.write_u8(0x00).unwrap();
let n = w.position();

// Receiver.
let frame = Delimited::bounded(&[0x00]).unwrap().next_frame(&wire[..n]).unwrap().unwrap();
let mut block = [0u8; 16];
let block = &mut block[..frame.payload().len()];
block.copy_from_slice(frame.payload());
let len = cobs::decode_in_place(block).unwrap();
let (body, crc) = block[..len].split_at(len - 2);
assert_eq!(ReadBuf::new(crc).read_u16_be().unwrap(), checksum::crc16_ccitt_false(body));
assert_eq!(body, &payload);
```

---

## End-to-end example

Length-prefix a varint-encoded record and decode it back, end to end:

```rust
use wire_codec::framing::{Endian, Framer, LengthPrefixed, LengthWidth};
use wire_codec::{Decode, Encode, ReadBuf, Result, WriteBuf, varint};

#[derive(PartialEq, Eq, Debug)]
struct Record<'a> { delta: i64, payload: &'a [u8] }

impl<'a> Encode for Record<'a> {
    fn encoded_size(&self) -> usize {
        varint::encoded_len_i64(self.delta)
            + varint::encoded_len_usize(self.payload.len())
            + self.payload.len()
    }
    fn encode(&self, buf: &mut WriteBuf<'_>) -> Result<()> {
        varint::encode_i64(self.delta, buf)?;
        varint::encode_usize(self.payload.len(), buf)?;
        buf.write_bytes(self.payload)
    }
}

impl<'de> Decode<'de> for Record<'de> {
    fn decode(buf: &mut ReadBuf<'de>) -> Result<Self> {
        let delta = varint::decode_i64(buf)?;
        let len = varint::decode_usize(buf)?;
        let payload = buf.read_bytes(len)?;
        Ok(Self { delta, payload })
    }
}

let record = Record { delta: -1234, payload: b"hello world" };

let mut inner = [0u8; 32];
let mut inner_buf = WriteBuf::new(&mut inner);
record.encode(&mut inner_buf).unwrap();
let inner_len = inner_buf.position();

let framer = LengthPrefixed::bounded(LengthWidth::U16, Endian::Big);
let mut wire = [0u8; 64];
let mut wire_buf = WriteBuf::new(&mut wire);
framer.write_frame(&inner[..inner_len], &mut wire_buf).unwrap();
let wire_len = wire_buf.position();

let frame = framer.next_frame(&wire[..wire_len]).unwrap().unwrap();
let mut read = ReadBuf::new(frame.payload());
let decoded = Record::decode(&mut read).unwrap();
assert_eq!(decoded, record);
```
