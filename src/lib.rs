//! # wire-codec
//!
//! A runtime-agnostic toolkit for binary framing and codec composition.
//!
//! `wire-codec` provides the primitives you need to build a binary protocol:
//! zero-copy buffer cursors, variable-length integer codecs, bit-level
//! cursors, and frame extraction strategies. It allocates nothing on the
//! encode or decode path, depends on nothing at runtime, and compiles on
//! `no_std` targets when the `std` feature is disabled.
//!
//! # When to reach for this crate
//!
//! Pick `wire-codec` when you need to:
//!
//! - Implement a custom binary protocol over any transport that delivers
//!   contiguous byte slices (TCP, UDP, in-memory queue, shared memory,
//!   serial link).
//! - Frame a byte stream into discrete messages using a length prefix or
//!   a delimiter, with bounded memory per frame.
//! - Encode and decode integers compactly via LEB128 varint and zigzag.
//! - Pack tagged fields into a record with bit-level precision.
//!
//! You probably want a higher-level serialization crate (`serde`, `prost`,
//! `bincode`) if you need a derive macro, schema evolution, or
//! introspection. This crate is the layer underneath.
//!
//! # Module map
//!
//! | Module | Provides |
//! | ------ | -------- |
//! | [`buf`] | [`ReadBuf`] and [`WriteBuf`], the zero-copy byte cursors every other module is built on. |
//! | [`error`] | [`Error`] and [`Result`], the crate-wide error contract. |
//! | [`traits`] | [`Encode`] and [`Decode`], the codec trait pair. |
//! | [`varint`] | Unsigned LEB128 varint for `u32` and `u64`. |
//! | [`zigzag`] | Signed-to-unsigned mapping for compact signed varints. |
//! | [`bitfield`] | [`BitReader`] and [`BitWriter`] for MSB-first packed bits. |
//! | [`framing`] | The [`Framer`][`framing::Framer`] trait plus [`LengthPrefixed`][`framing::LengthPrefixed`] and [`Delimited`][`framing::Delimited`] strategies. |
//!
//! # Example
//!
//! Length-prefix a payload, then read it back:
//!
//! ```
//! use wire_codec::WriteBuf;
//! use wire_codec::framing::{Endian, Framer, LengthPrefixed, LengthWidth};
//!
//! let framer = LengthPrefixed::new(LengthWidth::U16, Endian::Big);
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
//! # Stability
//!
//! From `1.0.0` onward, the public API is frozen. Subsequent `1.x` releases
//! add only bug fixes, performance improvements, and non-breaking additions:
//!
//! - New [`Error`] variants ([`Error`] is `#[non_exhaustive]`).
//! - New [`framing::LengthWidth`] widths ([`framing::LengthWidth`] is
//!   `#[non_exhaustive]`).
//! - New methods on existing types.
//! - Additional features behind feature flags.
//!
//! Any breaking change requires a `2.0` release.
//!
//! # Feature flags
//!
//! | Feature | Default | Effect |
//! | ------- | ------- | ------ |
//! | `std`   | yes     | Adds `impl std::error::Error for Error`. Drop this feature for `no_std` targets. |
//!
//! # License
//!
//! Dual-licensed under Apache-2.0 OR MIT.

#![doc(html_root_url = "https://docs.rs/wire-codec")]
#![cfg_attr(docsrs, feature(doc_cfg))]
#![cfg_attr(not(feature = "std"), no_std)]
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

pub mod bitfield;
pub mod buf;
pub mod error;
pub mod framing;
pub mod traits;
pub mod varint;
pub mod zigzag;

pub use bitfield::{BitReader, BitWriter};
pub use buf::{ReadBuf, WriteBuf};
pub use error::{Error, Result};
pub use traits::{Decode, Encode};

/// Crate version string, populated by Cargo at build time.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
