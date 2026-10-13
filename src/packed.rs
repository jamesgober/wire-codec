//! Packed struct encoding.
//!
//! The [`packed_struct!`][`crate::packed_struct`] macro declares a struct
//! whose fields are packed, most-significant-bit first, into a fixed number
//! of bytes, and implements [`Encode`][`crate::Encode`] and
//! [`Decode`][`crate::Decode`] for it. Each field names its width in bits:
//!
//! ```
//! use wire_codec::{packed_struct, Decode, Encode, ReadBuf, WriteBuf};
//!
//! packed_struct! {
//!     /// An IPv4 header's first 32 bits.
//!     #[derive(Debug, Clone, Copy, PartialEq, Eq)]
//!     pub struct Ipv4Head {
//!         pub version: u8 = 4,
//!         pub ihl: u8 = 4,
//!         pub dscp: u8 = 6,
//!         pub ecn: u8 = 2,
//!         pub total_length: u16 = 16,
//!     }
//! }
//!
//! assert_eq!(Ipv4Head::BITS, 32);
//! assert_eq!(Ipv4Head::SIZE, 4);
//!
//! let head = Ipv4Head { version: 4, ihl: 5, dscp: 0, ecn: 0, total_length: 84 };
//! let mut out = [0u8; 4];
//! head.encode(&mut WriteBuf::new(&mut out)).unwrap();
//! assert_eq!(out, [0x45, 0x00, 0x00, 0x54]);
//!
//! let decoded = Ipv4Head::decode(&mut ReadBuf::new(&out)).unwrap();
//! assert_eq!(decoded, head);
//! ```
//!
//! # Field types
//!
//! Any type implementing [`PackedField`] can be a field: all unsigned and
//! signed integers up to 64 bits (signed fields use two's complement in their
//! declared width) and `bool` (one bit). Implement [`PackedField`] on your
//! own enums to pack them directly.
//!
//! # Rules
//!
//! - Field widths are checked at compile time: a width of `0`, or wider than
//!   the field type (for example `u8 = 9`), is a compile error.
//! - Encoding a value that does not fit its width returns
//!   [`Error::BitOverflow`] and writes nothing.
//! - When the total width is not a multiple of eight, the last byte is padded
//!   with zero bits. Decoding rejects non-zero padding with
//!   [`Error::InvalidEncoding`], so every value has exactly one encoding.
//!   Declare reserved bits as an explicit field if a protocol lets them vary.
//! - Decoding is atomic: on error the cursor does not move.

use crate::error::{Error, Result};

/// A type that can be stored as a fixed-width bit field in a
/// [`packed_struct!`][`crate::packed_struct`].
///
/// # Example
///
/// ```
/// use wire_codec::packed::PackedField;
/// use wire_codec::{packed_struct, Decode, Error, ReadBuf, Result};
///
/// #[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// enum Kind { Data, Ack, Nak }
///
/// impl PackedField for Kind {
///     const MAX_BITS: u32 = 2;
///     fn to_bits(self, _bits: u32) -> Result<u64> {
///         Ok(self as u64)
///     }
///     fn from_bits(raw: u64, _bits: u32) -> Result<Self> {
///         match raw {
///             0 => Ok(Kind::Data),
///             1 => Ok(Kind::Ack),
///             2 => Ok(Kind::Nak),
///             _ => Err(Error::InvalidEncoding),
///         }
///     }
/// }
///
/// packed_struct! {
///     #[derive(Debug, PartialEq, Eq)]
///     struct Header {
///         kind: Kind = 2,
///         seq: u8 = 6,
///     }
/// }
///
/// let header = Header::decode(&mut ReadBuf::new(&[0b01_000011])).unwrap();
/// assert_eq!(header, Header { kind: Kind::Ack, seq: 3 });
/// ```
pub trait PackedField: Sized + Copy {
    /// The widest field, in bits, this type can be packed into.
    const MAX_BITS: u32;

    /// Convert `self` into the raw contents of a `bits`-wide field.
    ///
    /// # Errors
    ///
    /// Returns [`Error::BitOverflow`] if the value does not fit in `bits`
    /// bits.
    fn to_bits(self, bits: u32) -> Result<u64>;

    /// Rebuild a value from the raw contents of a `bits`-wide field.
    ///
    /// # Errors
    ///
    /// Returns an error (typically [`Error::InvalidEncoding`]) if `raw` is
    /// not a valid representation.
    fn from_bits(raw: u64, bits: u32) -> Result<Self>;
}

macro_rules! unsigned_fields {
    ($($ty:ty),*) => {$(
        impl PackedField for $ty {
            const MAX_BITS: u32 = <$ty>::BITS;

            #[inline]
            fn to_bits(self, bits: u32) -> Result<u64> {
                let raw = u64::from(self);
                if bits == 0 || bits > Self::MAX_BITS || (bits < 64 && raw >> bits != 0) {
                    return Err(Error::BitOverflow);
                }
                Ok(raw)
            }

            #[inline]
            fn from_bits(raw: u64, _bits: u32) -> Result<Self> {
                <$ty>::try_from(raw).map_err(|_| Error::BitOverflow)
            }
        }
    )*};
}

macro_rules! signed_fields {
    ($($ty:ty),*) => {$(
        impl PackedField for $ty {
            const MAX_BITS: u32 = <$ty>::BITS;

            #[inline]
            fn to_bits(self, bits: u32) -> Result<u64> {
                if bits > Self::MAX_BITS {
                    return Err(Error::BitOverflow);
                }
                crate::bitfield::signed_to_bits(i64::from(self), bits).ok_or(Error::BitOverflow)
            }

            #[inline]
            fn from_bits(raw: u64, bits: u32) -> Result<Self> {
                if bits == 0 || bits > Self::MAX_BITS {
                    return Err(Error::BitOverflow);
                }
                let value = crate::bitfield::sign_extend(raw, bits);
                <$ty>::try_from(value).map_err(|_| Error::BitOverflow)
            }
        }
    )*};
}

unsigned_fields!(u8, u16, u32, u64);
signed_fields!(i8, i16, i32, i64);

impl PackedField for bool {
    const MAX_BITS: u32 = 1;

    #[inline]
    fn to_bits(self, _bits: u32) -> Result<u64> {
        Ok(u64::from(self))
    }

    #[inline]
    fn from_bits(raw: u64, _bits: u32) -> Result<Self> {
        match raw {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(Error::InvalidEncoding),
        }
    }
}

/// Declare a bit-packed struct and implement [`Encode`][`crate::Encode`] and
/// [`Decode`][`crate::Decode`] for it.
///
/// Syntax: an ordinary struct definition in which every field is followed by
/// `= <width in bits>`. Attributes, doc comments, and visibility on the
/// struct and on each field are passed through. Fields are packed in
/// declaration order, most-significant bit first, using
/// [`BitWriter`][`crate::BitWriter`] / [`BitReader`][`crate::BitReader`].
///
/// The macro adds two associated constants, `BITS` (total field width) and
/// `SIZE` (encoded bytes), and two convenience methods, `pack` and
/// `unpack`, that work on a `[u8; SIZE]` array. See the
/// [`packed`][`crate::packed`] module for the full rules.
///
/// # Example
///
/// ```
/// use wire_codec::packed_struct;
///
/// packed_struct! {
///     #[derive(Debug, Clone, Copy, PartialEq, Eq)]
///     pub struct Sample {
///         pub valid: bool = 1,
///         pub channel: u8 = 3,
///         pub delta: i16 = 12,
///     }
/// }
///
/// let sample = Sample { valid: true, channel: 5, delta: -2 };
/// let bytes = sample.pack().unwrap();
/// assert_eq!(bytes, [0b1_101_1111, 0b1111_1110]);
/// assert_eq!(Sample::unpack(&bytes).unwrap(), sample);
/// ```
///
/// A field wider than its type does not compile:
///
/// ```compile_fail
/// wire_codec::packed_struct! {
///     struct Bad { x: u8 = 9 }
/// }
/// ```
#[macro_export]
macro_rules! packed_struct {
    (
        $(#[$meta:meta])*
        $vis:vis struct $name:ident {
            $(
                $(#[$fmeta:meta])*
                $fvis:vis $field:ident : $ty:ty = $bits:expr
            ),+ $(,)?
        }
    ) => {
        $(#[$meta])*
        $vis struct $name {
            $(
                $(#[$fmeta])*
                $fvis $field: $ty,
            )+
        }

        const _: () = {
            $(
                assert!(
                    ($bits) >= 1 && ($bits) <= <$ty as $crate::packed::PackedField>::MAX_BITS,
                    concat!(
                        "packed_struct!: field `",
                        stringify!($field),
                        "` has a width of zero or wider than its type"
                    )
                );
            )+
        };

        impl $name {
            /// Total width of the packed fields, in bits.
            pub const BITS: u32 = 0 $(+ ($bits))+;

            /// Encoded size in bytes: [`Self::BITS`] rounded up to a whole byte.
            pub const SIZE: usize = ($name::BITS as usize).div_ceil(8);

            /// Pack `self` into a byte array.
            ///
            /// # Errors
            ///
            /// Returns `Error::BitOverflow` if a field value does not fit in
            /// its declared width.
            #[allow(dead_code)]
            pub fn pack(&self) -> $crate::Result<[u8; $name::SIZE]> {
                let mut bytes = [0u8; $name::SIZE];
                let mut writer = $crate::BitWriter::new(&mut bytes);
                $(
                    writer.write_bits(
                        $crate::packed::PackedField::to_bits(self.$field, $bits)?,
                        $bits,
                    )?;
                )+
                let _written = writer.finish();
                Ok(bytes)
            }

            /// Unpack a value from a byte array.
            ///
            /// # Errors
            ///
            /// Returns `Error::InvalidEncoding` if the padding bits are not
            /// zero or a field rejects its raw value.
            #[allow(dead_code)]
            pub fn unpack(bytes: &[u8; $name::SIZE]) -> $crate::Result<Self> {
                let mut reader = $crate::BitReader::new(bytes);
                $(
                    let $field = <$ty as $crate::packed::PackedField>::from_bits(
                        reader.read_bits($bits)?,
                        $bits,
                    )?;
                )+
                let padding = reader.bits_remaining();
                if padding > 0 && reader.read_bits(padding as u32)? != 0 {
                    return Err($crate::Error::InvalidEncoding);
                }
                Ok(Self { $($field),+ })
            }
        }

        impl $crate::Encode for $name {
            #[inline]
            fn encoded_size(&self) -> usize {
                $name::SIZE
            }

            fn encode(&self, buf: &mut $crate::WriteBuf<'_>) -> $crate::Result<()> {
                buf.write_bytes(&self.pack()?)
            }
        }

        impl<'de> $crate::Decode<'de> for $name {
            fn decode(buf: &mut $crate::ReadBuf<'de>) -> $crate::Result<Self> {
                let mut probe = buf.clone();
                let value = $name::unpack(probe.read_array_ref::<{ $name::SIZE }>()?)?;
                *buf = probe;
                Ok(value)
            }
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Decode, Encode, ReadBuf, WriteBuf};

    crate::packed_struct! {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        struct Mixed {
            flag: bool = 1,
            small: u8 = 3,
            signed: i8 = 4,
            wide: u32 = 20,
            neg: i64 = 64,
            tail: u16 = 3,
        }
    }

    #[test]
    fn layout_constants() {
        assert_eq!(Mixed::BITS, 1 + 3 + 4 + 20 + 64 + 3);
        assert_eq!(Mixed::SIZE, 12);
    }

    #[test]
    fn round_trip_and_padding() {
        let value = Mixed {
            flag: true,
            small: 7,
            signed: -8,
            wide: 0xF_FFFF,
            neg: i64::MIN,
            tail: 5,
        };
        let mut out = [0xEEu8; 16];
        let mut buf = WriteBuf::new(&mut out);
        value.encode(&mut buf).unwrap();
        assert_eq!(buf.position(), Mixed::SIZE);
        assert_eq!(value.encoded_size(), Mixed::SIZE);
        // 95 bits: the final bit of byte 12 is zero padding.
        assert_eq!(out[11] & 0b1, 0);

        let mut r = ReadBuf::new(&out[..Mixed::SIZE]);
        assert_eq!(Mixed::decode(&mut r).unwrap(), value);
        assert!(r.is_empty());

        let mut bad = out;
        bad[11] |= 1;
        let mut r = ReadBuf::new(&bad[..Mixed::SIZE]);
        assert_eq!(Mixed::decode(&mut r), Err(Error::InvalidEncoding));
        assert_eq!(r.position(), 0, "decode is atomic");
    }

    #[test]
    fn out_of_range_values_are_rejected() {
        let mut value = Mixed {
            flag: false,
            small: 8,
            signed: 0,
            wide: 0,
            neg: 0,
            tail: 0,
        };
        let mut out = [0u8; 16];
        let mut buf = WriteBuf::new(&mut out);
        assert_eq!(value.encode(&mut buf), Err(Error::BitOverflow));
        value.small = 0;
        value.signed = 8;
        assert_eq!(value.encode(&mut buf), Err(Error::BitOverflow));
        value.signed = -9;
        assert_eq!(value.pack(), Err(Error::BitOverflow));
        assert_eq!(buf.position(), 0);
    }

    #[test]
    fn short_input_is_eof() {
        let mut r = ReadBuf::new(&[0u8; 11]);
        assert_eq!(Mixed::decode(&mut r), Err(Error::UnexpectedEof));
    }

    #[test]
    fn primitive_field_conversions() {
        assert_eq!(u8::from_bits(256, 8), Err(Error::BitOverflow));
        assert_eq!(u64::to_bits(u64::MAX, 64), Ok(u64::MAX));
        assert_eq!(u16::to_bits(1, 0), Err(Error::BitOverflow));
        assert_eq!(u16::to_bits(1, 17), Err(Error::BitOverflow));
        assert_eq!(i8::to_bits(-1, 9), Err(Error::BitOverflow));
        assert_eq!(i8::from_bits(0xFF, 0), Err(Error::BitOverflow));
        assert_eq!(i8::from_bits(0xFF, 9), Err(Error::BitOverflow));
        assert_eq!(i16::from_bits(0b111, 3), Ok(-1));
        assert_eq!(i32::to_bits(-1, 32), Ok(0xFFFF_FFFF));
        assert_eq!(i64::from_bits(1, 64), Ok(1));
        assert_eq!(bool::from_bits(2, 1), Err(Error::InvalidEncoding));
        assert_eq!(bool::to_bits(true, 1), Ok(1));
        assert_eq!(u32::from_bits(5, 3), Ok(5));
    }
}
