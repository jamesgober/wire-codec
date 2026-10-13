//! Error type returned by codec and framing operations.
//!
//! All fallible APIs in this crate return [`Result<T>`][`crate::Result`], which is
//! [`core::result::Result<T, Error>`]. The [`Error`] enum is `#[non_exhaustive]`
//! so future variants can be added without breaking semver.
//!
//! # Cursor state on error
//!
//! Every read, write, encode, decode, and frame operation in this crate is
//! atomic: when it returns `Err`, the [`ReadBuf`][`crate::ReadBuf`] or
//! [`WriteBuf`][`crate::WriteBuf`] it was given is left at the position it
//! had before the call. Use [`ReadBuf::position`][`crate::ReadBuf::position`]
//! to report where in the input a failure occurred.

use core::fmt;

/// Errors returned by codec and framing operations.
///
/// The variants fall into three groups:
///
/// - **Need more room or more data:** [`Error::UnexpectedEof`] and
///   [`Error::BufferFull`]. These are not protocol violations; a streaming
///   caller can retry once more input has arrived or the output has been
///   flushed.
/// - **Hostile or corrupt input:** [`Error::VarintOverflow`],
///   [`Error::FrameTooLarge`], [`Error::InvalidLengthPrefix`], and
///   [`Error::InvalidEncoding`]. The peer
///   sent bytes that violate the protocol. The usual response is to drop the
///   connection or discard the datagram; resynchronising inside a corrupted
///   stream is rarely possible.
/// - **Caller misuse:** [`Error::BitOverflow`] and [`Error::EmptyDelimiter`].
///   The local program asked for something the codec cannot represent. Fix
///   the call site.
///
/// # Example
///
/// ```
/// use wire_codec::{Error, ReadBuf};
///
/// let mut buf = ReadBuf::new(&[0x01]);
/// // Only one byte available but we ask for two.
/// let result = buf.read_u16_be();
/// assert!(matches!(result, Err(Error::UnexpectedEof)));
/// assert_eq!(buf.position(), 0, "the cursor did not move");
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// Reader ran out of bytes before the requested value could be decoded.
    ///
    /// When decoding a stream, wait for more input and retry the same call.
    /// When decoding a complete message, the message is truncated.
    UnexpectedEof,
    /// Writer ran out of capacity before the requested value could be encoded.
    ///
    /// Flush or grow the output buffer and retry. Nothing was written.
    BufferFull,
    /// A varint exceeded the maximum encoded length for its target width, or
    /// its value does not fit in the target type.
    ///
    /// The input is corrupt or hostile.
    VarintOverflow,
    /// A frame's length exceeds the configured limit for the framer.
    ///
    /// On decode, the peer announced (or, for delimited framing, has already
    /// sent at least) `len` payload bytes against a limit of `limit`. Treat it
    /// as hostile input. On encode, the local payload is larger than the
    /// framer allows. Both fields saturate at `usize::MAX`.
    FrameTooLarge {
        /// Frame payload length advertised by the protocol, or a lower bound
        /// on it when the exact length is not yet known.
        len: usize,
        /// Configured upper bound enforced by the framer.
        limit: usize,
    },
    /// A length prefix was structurally invalid.
    ///
    /// Not produced by the built-in framers in this release. Available to
    /// custom [`Framer`][`crate::framing::Framer`] implementations.
    InvalidLengthPrefix,
    /// A delimiter byte sequence was not found within the available input.
    ///
    /// Not produced by the built-in framers, which report an over-long
    /// undelimited input as [`Error::FrameTooLarge`]. Available to custom
    /// [`Framer`][`crate::framing::Framer`] implementations.
    DelimiterNotFound,
    /// Decoded data violated a structural invariant required by the codec.
    ///
    /// Produced by the canonical varint decoders for overlong encodings. The
    /// input is corrupt or hostile.
    InvalidEncoding,
    /// A bit-level write requested more bits than the target type can hold,
    /// or a value did not fit in the requested bit width.
    BitOverflow,
    /// A delimited-framer constructor was given an empty delimiter, which
    /// cannot uniquely separate frames.
    EmptyDelimiter,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Error::UnexpectedEof => f.write_str("unexpected end of input"),
            Error::BufferFull => f.write_str("output buffer is full"),
            Error::VarintOverflow => f.write_str("varint encoded length exceeded the target width"),
            Error::FrameTooLarge { len, limit } => {
                write!(f, "frame length {len} exceeds configured limit {limit}")
            }
            Error::InvalidLengthPrefix => f.write_str("length prefix is structurally invalid"),
            Error::DelimiterNotFound => f.write_str("delimiter not found in input"),
            Error::InvalidEncoding => f.write_str("encoded data violates a structural invariant"),
            Error::BitOverflow => f.write_str("bit width or value out of range"),
            Error::EmptyDelimiter => f.write_str("delimiter must be non-empty"),
        }
    }
}

/// Implemented unconditionally, including on `no_std` targets.
/// `std::error::Error` is the same trait, so `Error` converts into
/// `Box<dyn std::error::Error>`, `anyhow::Error`, and friends whether or not
/// the `std` feature is enabled.
impl core::error::Error for Error {}

/// Result alias used throughout the crate.
pub type Result<T> = core::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_messages_are_distinct_and_non_empty() {
        let all = [
            Error::UnexpectedEof,
            Error::BufferFull,
            Error::VarintOverflow,
            Error::FrameTooLarge { len: 9, limit: 4 },
            Error::InvalidLengthPrefix,
            Error::DelimiterNotFound,
            Error::InvalidEncoding,
            Error::BitOverflow,
            Error::EmptyDelimiter,
        ];
        let mut seen: [Option<String>; 9] = Default::default();
        for (slot, err) in seen.iter_mut().zip(all) {
            let text = err.to_string();
            assert!(!text.is_empty());
            *slot = Some(text);
        }
        for i in 0..seen.len() {
            for j in (i + 1)..seen.len() {
                assert_ne!(seen[i], seen[j]);
            }
        }
        assert_eq!(
            Error::FrameTooLarge { len: 9, limit: 4 }.to_string(),
            "frame length 9 exceeds configured limit 4"
        );
    }

    #[test]
    fn implements_core_error() {
        fn takes_error(_: &dyn core::error::Error) {}
        takes_error(&Error::BufferFull);
    }
}
