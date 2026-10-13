//! Core traits implemented by every encodable value.
//!
//! [`Encode`] writes a value into a [`WriteBuf`]; [`Decode`] reads one out of a
//! [`ReadBuf`]. The traits are deliberately minimal so callers can compose them
//! with any framing strategy in the [`framing`][`crate::framing`] module.
//!
//! # Provided implementations
//!
//! | Type | Wire form |
//! | ---- | --------- |
//! | `u8`, `i8` | one byte |
//! | `bool` | one byte, `0` or `1`; any other byte decodes as [`Error::InvalidEncoding`] |
//! | `[u8; N]` | `N` raw bytes (copied on decode) |
//! | `&'de [u8; N]` | `N` raw bytes (zero-copy decode) |
//! | `[u8]` | raw bytes, encode only (the length must be framed separately) |
//! | [`Be<T>`] / [`Le<T>`] | `T` in big / little endian, for every integer from `u16` to `i128`, and `f32` / `f64` |
//! | [`Varint<T>`] | LEB128 varint for `u16`, `u32`, `u64`, `usize`; zigzag varint for `i32`, `i64` |
//! | `(A, B)` up to `(A, B, C, D, E, F)` | the elements in order |
//!
//! Multi-byte integers have no plain `Encode` impl on purpose: the byte order
//! is part of the wire format, so it must be chosen explicitly with a
//! wrapper.
//!
//! Tuples are atomic like every other operation in the crate: if a later
//! element fails, the cursor is restored to where the tuple started.

use crate::buf::{ReadBuf, WriteBuf};
use crate::error::{Error, Result};
use crate::varint;

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
    #[must_use]
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

impl Encode for i8 {
    #[inline]
    fn encoded_size(&self) -> usize {
        1
    }
    #[inline]
    fn encode(&self, buf: &mut WriteBuf<'_>) -> Result<()> {
        buf.write_i8(*self)
    }
}

impl<'de> Decode<'de> for i8 {
    #[inline]
    fn decode(buf: &mut ReadBuf<'de>) -> Result<Self> {
        buf.read_i8()
    }
}

impl Encode for bool {
    #[inline]
    fn encoded_size(&self) -> usize {
        1
    }
    #[inline]
    fn encode(&self, buf: &mut WriteBuf<'_>) -> Result<()> {
        buf.write_u8(u8::from(*self))
    }
}

/// Strict: only `0` and `1` are accepted, so every value has one encoding.
impl<'de> Decode<'de> for bool {
    #[inline]
    fn decode(buf: &mut ReadBuf<'de>) -> Result<Self> {
        let value = match buf.peek_u8()? {
            0 => false,
            1 => true,
            _ => return Err(Error::InvalidEncoding),
        };
        buf.consume(1);
        Ok(value)
    }
}

impl Encode for [u8] {
    #[inline]
    fn encoded_size(&self) -> usize {
        self.len()
    }
    #[inline]
    fn encode(&self, buf: &mut WriteBuf<'_>) -> Result<()> {
        buf.write_bytes(self)
    }
}

impl<const N: usize> Encode for [u8; N] {
    #[inline]
    fn encoded_size(&self) -> usize {
        N
    }
    #[inline]
    fn encode(&self, buf: &mut WriteBuf<'_>) -> Result<()> {
        buf.write_bytes(self)
    }
}

impl<'de, const N: usize> Decode<'de> for [u8; N] {
    #[inline]
    fn decode(buf: &mut ReadBuf<'de>) -> Result<Self> {
        buf.read_array()
    }
}

impl<'de, const N: usize> Decode<'de> for &'de [u8; N] {
    #[inline]
    fn decode(buf: &mut ReadBuf<'de>) -> Result<Self> {
        buf.read_array_ref()
    }
}

/// Big-endian wrapper: encodes and decodes `T` most significant byte first.
///
/// # Example
///
/// ```
/// use wire_codec::{Be, Decode, Encode, ReadBuf, WriteBuf};
///
/// let mut out = [0u8; 4];
/// let mut buf = WriteBuf::new(&mut out);
/// Be(0xCAFE_u16).encode(&mut buf).unwrap();
/// Be(-2_i16).encode(&mut buf).unwrap();
/// assert_eq!(out, [0xCA, 0xFE, 0xFF, 0xFE]);
///
/// let mut r = ReadBuf::new(&out);
/// assert_eq!(Be::<u16>::decode(&mut r).unwrap().0, 0xCAFE);
/// assert_eq!(Be::<i16>::decode(&mut r).unwrap(), Be(-2));
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Be<T>(pub T);

/// Little-endian wrapper: encodes and decodes `T` least significant byte
/// first.
///
/// # Example
///
/// ```
/// use wire_codec::{Decode, Encode, Le, ReadBuf, WriteBuf};
///
/// let mut out = [0u8; 4];
/// let mut buf = WriteBuf::new(&mut out);
/// Le(1.0_f32).encode(&mut buf).unwrap();
/// assert_eq!(out, [0x00, 0x00, 0x80, 0x3F]);
/// assert_eq!(Le::<f32>::decode(&mut ReadBuf::new(&out)).unwrap(), Le(1.0));
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Le<T>(pub T);

/// LEB128 varint wrapper. Signed integers use the zigzag mapping, matching
/// protobuf's `sint32` / `sint64`.
///
/// Decoding is lenient about overlong encodings, like
/// [`varint::decode_u64`]. Call the `varint::decode_*_canonical` functions
/// directly when canonical form must be enforced.
///
/// # Example
///
/// ```
/// use wire_codec::{Decode, Encode, ReadBuf, Varint, WriteBuf};
///
/// let mut out = [0u8; 8];
/// let mut buf = WriteBuf::new(&mut out);
/// Varint(300_u32).encode(&mut buf).unwrap();
/// Varint(-1_i64).encode(&mut buf).unwrap();
/// assert_eq!(buf.written(), &[0xAC, 0x02, 0x01]);
/// assert_eq!(Varint(300_u32).encoded_size(), 2);
///
/// let mut r = ReadBuf::new(&out[..3]);
/// assert_eq!(Varint::<u32>::decode(&mut r).unwrap(), Varint(300));
/// assert_eq!(Varint::<i64>::decode(&mut r).unwrap(), Varint(-1));
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Varint<T>(pub T);

macro_rules! fixed_width_impls {
    ($($ty:ty: $read_be:ident, $write_be:ident, $read_le:ident, $write_le:ident;)*) => {$(
        impl Encode for Be<$ty> {
            #[inline]
            fn encoded_size(&self) -> usize {
                core::mem::size_of::<$ty>()
            }
            #[inline]
            fn encode(&self, buf: &mut WriteBuf<'_>) -> Result<()> {
                buf.$write_be(self.0)
            }
        }

        impl<'de> Decode<'de> for Be<$ty> {
            #[inline]
            fn decode(buf: &mut ReadBuf<'de>) -> Result<Self> {
                buf.$read_be().map(Be)
            }
        }

        impl Encode for Le<$ty> {
            #[inline]
            fn encoded_size(&self) -> usize {
                core::mem::size_of::<$ty>()
            }
            #[inline]
            fn encode(&self, buf: &mut WriteBuf<'_>) -> Result<()> {
                buf.$write_le(self.0)
            }
        }

        impl<'de> Decode<'de> for Le<$ty> {
            #[inline]
            fn decode(buf: &mut ReadBuf<'de>) -> Result<Self> {
                buf.$read_le().map(Le)
            }
        }
    )*};
}

fixed_width_impls! {
    u16: read_u16_be, write_u16_be, read_u16_le, write_u16_le;
    u32: read_u32_be, write_u32_be, read_u32_le, write_u32_le;
    u64: read_u64_be, write_u64_be, read_u64_le, write_u64_le;
    u128: read_u128_be, write_u128_be, read_u128_le, write_u128_le;
    i16: read_i16_be, write_i16_be, read_i16_le, write_i16_le;
    i32: read_i32_be, write_i32_be, read_i32_le, write_i32_le;
    i64: read_i64_be, write_i64_be, read_i64_le, write_i64_le;
    i128: read_i128_be, write_i128_be, read_i128_le, write_i128_le;
    f32: read_f32_be, write_f32_be, read_f32_le, write_f32_le;
    f64: read_f64_be, write_f64_be, read_f64_le, write_f64_le;
}

macro_rules! varint_impls {
    ($($ty:ty: $len:path, $encode:path, $decode:path;)*) => {$(
        impl Encode for Varint<$ty> {
            #[inline]
            fn encoded_size(&self) -> usize {
                $len(self.0)
            }
            #[inline]
            fn encode(&self, buf: &mut WriteBuf<'_>) -> Result<()> {
                $encode(self.0, buf)
            }
        }

        impl<'de> Decode<'de> for Varint<$ty> {
            #[inline]
            fn decode(buf: &mut ReadBuf<'de>) -> Result<Self> {
                $decode(buf).map(Varint)
            }
        }
    )*};
}

varint_impls! {
    u16: varint::encoded_len_u16, varint::encode_u16, varint::decode_u16;
    u32: varint::encoded_len_u32, varint::encode_u32, varint::decode_u32;
    u64: varint::encoded_len_u64, varint::encode_u64, varint::decode_u64;
    usize: varint::encoded_len_usize, varint::encode_usize, varint::decode_usize;
    i32: varint::encoded_len_i32, varint::encode_i32, varint::decode_i32;
    i64: varint::encoded_len_i64, varint::encode_i64, varint::decode_i64;
}

macro_rules! tuple_impls {
    ($(($($name:ident $idx:tt),+);)*) => {$(
        impl<$($name: Encode),+> Encode for ($($name,)+) {
            #[inline]
            fn encoded_size(&self) -> usize {
                0 $(+ self.$idx.encoded_size())+
            }
            #[inline]
            fn encode(&self, buf: &mut WriteBuf<'_>) -> Result<()> {
                let start = buf.position();
                let result = (|| {
                    $(self.$idx.encode(buf)?;)+
                    Ok(())
                })();
                if result.is_err() {
                    buf.rewind(start);
                }
                result
            }
        }

        impl<'de, $($name: Decode<'de>),+> Decode<'de> for ($($name,)+) {
            #[inline]
            fn decode(buf: &mut ReadBuf<'de>) -> Result<Self> {
                let mut scratch = buf.clone();
                let value = ($($name::decode(&mut scratch)?,)+);
                *buf = scratch;
                Ok(value)
            }
        }
    )*};
}

tuple_impls! {
    (A 0, B 1);
    (A 0, B 1, C 2);
    (A 0, B 1, C 2, D 3);
    (A 0, B 1, C 2, D 3, E 4);
    (A 0, B 1, C 2, D 3, E 4, F 5);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip<T>(value: T) -> T
    where
        T: Encode + for<'de> Decode<'de> + PartialEq + core::fmt::Debug,
    {
        let mut out = [0u8; 64];
        let mut buf = WriteBuf::new(&mut out);
        value.encode(&mut buf).unwrap();
        let n = buf.position();
        assert_eq!(n, value.encoded_size(), "encoded_size must be exact");
        let mut r = ReadBuf::new(&out[..n]);
        let decoded = T::decode(&mut r).unwrap();
        assert!(r.is_empty());
        decoded
    }

    #[test]
    fn primitive_round_trips() {
        assert_eq!(round_trip(0xABu8), 0xAB);
        assert_eq!(round_trip(-5i8), -5);
        assert!(round_trip(true));
        assert!(!round_trip(false));
        assert_eq!(round_trip([1u8, 2, 3]), [1, 2, 3]);
    }

    #[test]
    fn wrapper_round_trips() {
        assert_eq!(round_trip(Be(u16::MAX)), Be(u16::MAX));
        assert_eq!(round_trip(Le(0x0102_0304_u32)), Le(0x0102_0304));
        assert_eq!(round_trip(Be(u64::MAX - 1)), Be(u64::MAX - 1));
        assert_eq!(round_trip(Le(u128::MAX / 3)), Le(u128::MAX / 3));
        assert_eq!(round_trip(Be(i32::MIN)), Be(i32::MIN));
        assert_eq!(round_trip(Le(i64::MIN)), Le(i64::MIN));
        assert_eq!(round_trip(Be(i128::MIN)), Be(i128::MIN));
        assert_eq!(round_trip(Le(-3i16)), Le(-3));
        assert_eq!(round_trip(Be(2.5f64)), Be(2.5));
        assert_eq!(round_trip(Le(-0.5f64)), Le(-0.5));
        assert_eq!(round_trip(Be(9.0f32)), Be(9.0));
        for v in [0u16, 300, u16::MAX] {
            assert_eq!(round_trip(Varint(v)), Varint(v));
        }
        for v in [0usize, 1 << 20, usize::MAX] {
            assert_eq!(round_trip(Varint(v)), Varint(v));
        }
        assert_eq!(round_trip(Varint(u32::MAX)), Varint(u32::MAX));
        assert_eq!(round_trip(Varint(u64::MAX)), Varint(u64::MAX));
        assert_eq!(round_trip(Varint(i32::MIN)), Varint(i32::MIN));
        assert_eq!(round_trip(Varint(i64::MAX)), Varint(i64::MAX));
    }

    #[test]
    fn byte_order_is_explicit() {
        let mut out = [0u8; 4];
        let mut buf = WriteBuf::new(&mut out);
        Be(0x0102u16).encode(&mut buf).unwrap();
        Le(0x0102u16).encode(&mut buf).unwrap();
        assert_eq!(out, [0x01, 0x02, 0x02, 0x01]);
    }

    #[test]
    fn tuples_encode_in_order() {
        let value = (1u8, Be(0x0203u16), Varint(300u32), true);
        let mut out = [0u8; 16];
        let mut buf = WriteBuf::new(&mut out);
        value.encode(&mut buf).unwrap();
        assert_eq!(buf.written(), &[1, 2, 3, 0xAC, 0x02, 1]);
        assert_eq!(round_trip(value), value);
        assert_eq!(round_trip((1u8, 2u8)), (1, 2));
        assert_eq!(round_trip((1u8, 2u8, 3u8)), (1, 2, 3));
        assert_eq!(round_trip((1u8, 2u8, 3u8, 4u8, 5u8)), (1, 2, 3, 4, 5));
        assert_eq!(
            round_trip((1u8, 2u8, 3u8, 4u8, 5u8, 6u8)),
            (1, 2, 3, 4, 5, 6)
        );
    }

    #[test]
    fn tuple_decode_and_encode_are_atomic() {
        // Second element is short: the cursor must not keep the first byte.
        let mut r = ReadBuf::new(&[7, 0x01]);
        assert_eq!(<(u8, Be<u16>)>::decode(&mut r), Err(Error::UnexpectedEof));
        assert_eq!(r.position(), 0);

        let mut out = [0u8; 2];
        let mut buf = WriteBuf::new(&mut out);
        assert_eq!((1u8, Be(2u16)).encode(&mut buf), Err(Error::BufferFull));
        assert_eq!(buf.position(), 0);
    }

    #[test]
    fn bool_is_strict() {
        let mut r = ReadBuf::new(&[2]);
        assert_eq!(bool::decode(&mut r), Err(Error::InvalidEncoding));
        assert_eq!(r.position(), 0);
        assert_eq!(
            bool::decode(&mut ReadBuf::new(&[])),
            Err(Error::UnexpectedEof)
        );
    }

    #[test]
    fn byte_slices_and_array_refs() {
        let mut out = [0u8; 8];
        let mut buf = WriteBuf::new(&mut out);
        let raw: &[u8] = b"wire";
        assert_eq!(raw.encoded_size(), 4);
        raw.encode(&mut buf).unwrap();
        assert_eq!(buf.written(), b"wire");

        let data = *b"abcdef";
        let mut r = ReadBuf::new(&data);
        let head = <&[u8; 4]>::decode(&mut r).unwrap();
        assert_eq!(head, b"abcd");
        assert_eq!(<[u8; 4]>::decode(&mut r), Err(Error::UnexpectedEof));
    }

    #[test]
    fn encode_errors_propagate() {
        let mut out = [0u8; 1];
        let mut buf = WriteBuf::new(&mut out);
        assert_eq!(Be(1u32).encode(&mut buf), Err(Error::BufferFull));
        assert_eq!(Le(1u32).encode(&mut buf), Err(Error::BufferFull));
        assert_eq!(Varint(300u32).encode(&mut buf), Err(Error::BufferFull));
        assert_eq!(buf.position(), 0);
        let mut r = ReadBuf::new(&[0x80]);
        assert_eq!(Varint::<u64>::decode(&mut r), Err(Error::UnexpectedEof));
        assert_eq!(Le::<u16>::decode(&mut r), Err(Error::UnexpectedEof));
        assert_eq!(Be::<u16>::decode(&mut r), Err(Error::UnexpectedEof));
    }
}
