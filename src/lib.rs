//! # wire-codec
//!
//! A runtime-agnostic toolkit for binary framing and codec composition.
//!
//! `wire-codec` provides the primitives you need to build a binary protocol:
//! zero-copy buffer cursors, variable-length integer codecs, bit-level
//! cursors, bit-packed structs, checksums, byte stuffing, and frame
//! extraction strategies. It allocates nothing on the encode or decode path,
//! has no required runtime dependencies, and compiles on `no_std` targets
//! when the default `std` feature is disabled.
//!
//! # When to reach for this crate
//!
//! Pick `wire-codec` when you need to:
//!
//! - Implement a custom binary protocol over any transport that delivers
//!   contiguous byte slices (TCP, UDP, in-memory queue, shared memory,
//!   serial link).
//! - Frame a byte stream into discrete messages using a fixed-width or
//!   varint length prefix, or a delimiter, with a bounded payload size and
//!   linear-time streaming.
//! - Encode and decode integers compactly via LEB128 varint and zigzag.
//! - Pack fields into a record with bit-level precision, by hand with
//!   [`BitWriter`] / [`BitReader`] or declaratively with
//!   [`packed_struct!`].
//! - Add an integrity check (CRC-16, CRC-32, CRC-32C) to every frame, or
//!   COBS-stuff payloads for a serial link.
//!
//! You probably want a higher-level serialization crate (`serde`, `prost`,
//! `bincode`) if you need schema evolution or introspection. This crate is
//! the layer underneath.
//!
//! # Module map
//!
//! | Module | Provides |
//! | ------ | -------- |
//! | [`buf`] | [`ReadBuf`] and [`WriteBuf`], the zero-copy byte cursors every other module is built on. |
//! | [`error`] | [`Error`] and [`Result`], the crate-wide error contract. |
//! | [`traits`] | [`Encode`] and [`Decode`], the codec trait pair, with impls for primitives, arrays, tuples, and the [`Be`] / [`Le`] / [`Varint`] wrappers. |
//! | [`varint`] | LEB128 varint for `u16`, `u32`, `u64`, `usize`, and zigzag `i32` / `i64`, with canonical (strict) decoders. |
//! | [`zigzag`] | Signed-to-unsigned mapping for compact signed varints. |
//! | [`bitfield`] | [`BitReader`] and [`BitWriter`] for MSB-first packed bits. |
//! | [`packed`] | [`packed_struct!`] and the [`PackedField`][`packed::PackedField`] trait for bit-packed structs. |
//! | [`framing`] | The [`Framer`][`framing::Framer`] trait, [`StreamDecoder`][`framing::StreamDecoder`], and the [`LengthPrefixed`][`framing::LengthPrefixed`], [`VarintPrefixed`][`framing::VarintPrefixed`], [`Delimited`][`framing::Delimited`], and [`Checksummed`][`framing::Checksummed`] strategies. |
//! | [`checksum`] | The [`Checksum`][`checksum::Checksum`] trait and CRC-16/CCITT-FALSE, CRC-32, CRC-32C. |
//! | [`cobs`] | Consistent Overhead Byte Stuffing. |
//! | `codec` | `FrameCodec`, a `bytes::BytesMut` adapter with `tokio-util` and `asynchronous-codec` trait impls (optional features). |
//!
//! # Example
//!
//! Length-prefix a payload, then read it back:
//!
//! ```
//! use wire_codec::WriteBuf;
//! use wire_codec::framing::{Endian, Framer, LengthPrefixed, LengthWidth};
//!
//! let framer = LengthPrefixed::bounded(LengthWidth::U16, Endian::Big);
//!
//! let mut out = [0u8; 32];
//! let mut buf = WriteBuf::new(&mut out);
//! framer.write_frame(b"ping", &mut buf).unwrap();
//! let n = buf.position();
//!
//! let frame = framer.next_frame(&out[..n]).unwrap().unwrap();
//! assert_eq!(frame.payload(), b"ping");
//! ```
//!
//! # Error contract
//!
//! Every fallible operation returns [`Result`] and is atomic: on error the
//! cursor it was given has not moved and nothing was written. No public
//! function panics on any input, including on 32-bit targets, where every
//! length taken from the wire is converted with checked arithmetic.
//!
//! # Stability
//!
//! From `1.0.0` onward, the public API is frozen. Subsequent `1.x` releases
//! add only bug fixes, performance improvements, and non-breaking additions:
//!
//! - New [`Error`] variants ([`Error`] is `#[non_exhaustive]`).
//! - New [`framing::LengthWidth`] widths ([`framing::LengthWidth`] is
//!   `#[non_exhaustive]`).
//! - New methods on existing types, and new provided methods on traits.
//! - Additional features behind feature flags.
//!
//! Any breaking change requires a `2.0` release.
//!
//! The minimum supported Rust version (MSRV) is 1.85. Raising it is not
//! treated as a breaking change: it happens only in a minor release, is
//! called out in the changelog, and is never done in a patch release.
//!
//! # Feature flags
//!
//! | Feature | Default | Effect |
//! | ------- | ------- | ------ |
//! | `std`   | yes     | Links the standard library and adds `From<Error> for std::io::Error`. Drop it for `no_std` targets; [`Error`] implements `core::error::Error` either way. |
//! | `memchr` | no     | Delimiter search through the `memchr` crate (SIMD, linear-time multi-byte search) instead of the built-in SWAR scan. |
//! | `bytes` | no      | The `codec` module: `FrameCodec` over `bytes::BytesMut`. Works without `std`. |
//! | `tokio-util` | no | `tokio_util::codec::{Decoder, Encoder}` for `FrameCodec`. Implies `bytes` and `std`. |
//! | `asynchronous-codec` | no | `asynchronous_codec::{Decoder, Encoder}` for `FrameCodec`. Implies `bytes` and `std`. |
//!
//! # License
//!
//! Dual-licensed under Apache-2.0 OR MIT.

#![doc(html_root_url = "https://docs.rs/wire-codec")]
#![cfg_attr(docsrs, feature(doc_cfg))]
// Unit tests always link `std` (for `Vec`, `String`, formatting); the library
// itself is `no_std` whenever the `std` feature is off.
#![cfg_attr(not(any(feature = "std", test)), no_std)]
// REPS-mandated lint set. Keep aligned with REPS.md "Compiler & Lint Configuration".
#![deny(warnings)]
#![deny(missing_docs)]
#![deny(unsafe_op_in_unsafe_fn)]
#![deny(unused_must_use)]
#![deny(unused_results)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![deny(clippy::todo)]
#![deny(clippy::unimplemented)]
#![deny(clippy::unreachable)]
#![deny(clippy::print_stdout)]
#![deny(clippy::print_stderr)]
#![deny(clippy::dbg_macro)]
#![deny(clippy::undocumented_unsafe_blocks)]
#![deny(clippy::missing_safety_doc)]
// The crate contains no `unsafe` code; `forbid` makes that machine-checked.
#![forbid(unsafe_code)]

pub mod bitfield;
pub mod buf;
pub mod checksum;
pub mod cobs;
#[cfg(feature = "bytes")]
#[cfg_attr(docsrs, doc(cfg(feature = "bytes")))]
pub mod codec;
pub mod error;
pub mod framing;
pub mod packed;
pub mod traits;
pub mod varint;
pub mod zigzag;

pub use bitfield::{BitReader, BitWriter};
pub use buf::{ReadBuf, WriteBuf};
pub use error::{Error, Result};
pub use traits::{Be, Decode, Encode, Le, Varint};

/// Crate version string, populated by Cargo at build time.
///
/// # Example
///
/// ```
/// assert!(wire_codec::VERSION.starts_with("1."));
/// ```
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Compiles the README code blocks as doctests so they cannot drift.
#[cfg(doctest)]
#[doc = include_str!("../README.md")]
pub struct ReadmeDoctests;

/// Compiles the `docs/API.md` code blocks as doctests so they cannot drift.
#[cfg(doctest)]
#[doc = include_str!("../docs/API.md")]
pub struct ApiDoctests;
