//! Core traits implemented by every encodable value.
//!
//! [`Encode`] writes a value into a [`WriteBuf`]; [`Decode`] reads one out of a
//! [`ReadBuf`]. The traits are deliberately minimal so callers can compose them
//! with any framing strategy in the [`framing`][`crate::framing`] module.

use crate::buf::{ReadBuf, WriteBuf};
use crate::error::Result;

/// A value that can be written to a [`WriteBuf`].
///
/// Implementors must guarantee that [`Encode::encoded_size`] returns the exact
/// number of bytes [`Encode::encode`] writes on success.
///
/// # Example
///
/// ```
/// use wire_codec::{Encode, Result, WriteBuf};
///
/// struct Tagged {
///     tag: u8,
///     value: u32,
/// }
///
/// impl Encode for Tagged {
///     fn encoded_size(&self) -> usize { 5 }
///     fn encode(&self, buf: &mut WriteBuf<'_>) -> Result<()> {
///         buf.write_u8(self.tag)?;
///         buf.write_u32_be(self.value)
///     }
/// }
///
/// let mut out = [0u8; 5];
/// let mut buf = WriteBuf::new(&mut out);
/// Tagged { tag: 0x01, value: 0xCAFEBABE }.encode(&mut buf).unwrap();
/// assert_eq!(&out, &[0x01, 0xCA, 0xFE, 0xBA, 0xBE]);
/// ```
pub trait Encode {
    /// Number of bytes [`Encode::encode`] will write on success.
    fn encoded_size(&self) -> usize;

    /// Write `self` into `buf`, advancing its write position.
    ///
    /// # Errors
    ///
    /// Returns an error if `buf` lacks capacity for [`Encode::encoded_size`]
    /// bytes or if a sub-encoder reports an invariant violation.
    fn encode(&self, buf: &mut WriteBuf<'_>) -> Result<()>;
}

/// A value that can be read from a [`ReadBuf`].
///
/// The lifetime parameter `'de` ties the decoded value to the borrowed input,
/// enabling zero-copy decoding of borrowed payloads (`&'de [u8]`, `&'de str`).
///
/// # Example
///
/// ```
/// use wire_codec::{Decode, ReadBuf, Result};
///
/// struct Tagged {
///     tag: u8,
///     value: u32,
/// }
///
/// impl<'de> Decode<'de> for Tagged {
///     fn decode(buf: &mut ReadBuf<'de>) -> Result<Self> {
///         let tag = buf.read_u8()?;
///         let value = buf.read_u32_be()?;
///         Ok(Self { tag, value })
///     }
/// }
///
/// let mut buf = ReadBuf::new(&[0x01, 0xCA, 0xFE, 0xBA, 0xBE]);
/// let tagged = Tagged::decode(&mut buf).unwrap();
/// assert_eq!(tagged.tag, 0x01);
/// assert_eq!(tagged.value, 0xCAFEBABE);
/// ```
pub trait Decode<'de>: Sized {
    /// Read a value of `Self` from `buf`, advancing its read position.
    ///
    /// # Errors
    ///
    /// Returns an error if `buf` is exhausted before the value finishes, or if
    /// the encoded bytes violate the codec's structural rules.
    fn decode(buf: &mut ReadBuf<'de>) -> Result<Self>;
}

impl Encode for u8 {
    #[inline]
    fn encoded_size(&self) -> usize {
        1
    }
    #[inline]
    fn encode(&self, buf: &mut WriteBuf<'_>) -> Result<()> {
        buf.write_u8(*self)
    }
}

impl<'de> Decode<'de> for u8 {
    #[inline]
    fn decode(buf: &mut ReadBuf<'de>) -> Result<Self> {
        buf.read_u8()
    }
}
