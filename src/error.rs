//! Error type returned by codec and framing operations.
//!
//! All fallible APIs in this crate return [`Result<T>`][`crate::Result`], which is
//! [`core::result::Result<T, Error>`]. The [`Error`] enum is `#[non_exhaustive]`
//! so future variants can be added without breaking semver.

use core::fmt;

/// Errors returned by codec and framing operations.
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
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// Reader ran out of bytes before the requested value could be decoded.
    UnexpectedEof,
    /// Writer ran out of capacity before the requested value could be encoded.
    BufferFull,
    /// A varint exceeded the maximum encoded length for its target width.
    VarintOverflow,
    /// A frame's declared length exceeds the configured limit for the framer.
    FrameTooLarge {
        /// Frame length advertised by the protocol.
        len: usize,
        /// Configured upper bound enforced by the framer.
        limit: usize,
    },
    /// A length prefix was structurally invalid (e.g. width zero, or a width
    /// larger than the codec supports).
    InvalidLengthPrefix,
    /// A delimiter byte sequence was not found within the available input.
    DelimiterNotFound,
    /// Decoded data violated a structural invariant required by the codec.
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

#[cfg(feature = "std")]
impl std::error::Error for Error {}

/// Result alias used throughout the crate.
pub type Result<T> = core::result::Result<T, Error>;
