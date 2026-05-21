//! # wire-codec
//!
//! Binary frame codec and protocol codec toolkit. Length-prefixed, delimiter-
//! based, and custom framing strategies. Built-in varint, zigzag, and bitfield
//! encoding. Runtime-agnostic foundation under network-protocol crates.
//!
//! # Module map
//!
//! - [`buf`] holds [`ReadBuf`] and [`WriteBuf`], the zero-copy byte cursors
//!   every other module is built on.
//! - [`traits`] defines [`Encode`] and [`Decode`], the codec trait pair.
//! - [`varint`] and [`zigzag`] provide variable-length integer primitives.
//! - [`bitfield`] provides packed-bit read and write cursors.
//! - [`framing`] supplies a [`Framer`][`framing::Framer`] trait plus
//!   [`LengthPrefixed`][`framing::LengthPrefixed`] and
//!   [`Delimited`][`framing::Delimited`] strategies.
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
//! # Status
//!
//! Pre-1.0 foundation. Public API surface defined in this release; further
//! milestones expand implementations and lock semantics in.
//!
//! # License
//!
//! Dual-licensed under Apache-2.0 OR MIT.

#![doc(html_root_url = "https://docs.rs/wire-codec")]
#![cfg_attr(docsrs, feature(doc_cfg))]
#![cfg_attr(not(feature = "std"), no_std)]
#![deny(missing_docs)]
#![deny(unsafe_op_in_unsafe_fn)]
#![deny(unused_must_use)]
#![deny(unused_results)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![deny(clippy::todo)]
#![deny(clippy::unimplemented)]
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
