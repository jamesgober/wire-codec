//! Unsigned LEB128 varint encoding, plus zigzag-mapped signed helpers.
//!
//! A varint encodes an unsigned integer in one to ten bytes. The low seven
//! bits of each byte carry payload, and the top bit signals continuation:
//! `1` means more bytes follow, `0` means the value is complete.
//!
//! The encoding is identical to the format used by Protocol Buffers, WebAssembly,
//! and DWARF. The signed helpers ([`encode_i32`], [`decode_i64`], ...) apply
//! the [`zigzag`][`crate::zigzag`] mapping first, matching protobuf's
//! `sint32` / `sint64` wire types.
//!
//! # Error contract
//!
//! Every function in this module is atomic: on error the cursor is left
//! exactly where it was. A streaming caller that receives
//! [`Error::UnexpectedEof`] can append more bytes and retry the same call
//! without cloning the cursor first.

use crate::buf::{ReadBuf, WriteBuf};
use crate::error::{Error, Result};
use crate::zigzag;

/// Maximum encoded length of a `u16` varint.
pub const MAX_LEN_U16: usize = 3;
/// Maximum encoded length of a `u32` varint.
pub const MAX_LEN_U32: usize = 5;
/// Maximum encoded length of a `u64` varint.
pub const MAX_LEN_U64: usize = 10;

/// Encoded length of `value` as a `u16` varint, in bytes (1 to 3).
///
/// # Example
///
/// ```
/// use wire_codec::varint;
/// assert_eq!(varint::encoded_len_u16(127), 1);
/// assert_eq!(varint::encoded_len_u16(u16::MAX), 3);
/// ```
#[inline]
#[must_use]
pub const fn encoded_len_u16(value: u16) -> usize {
    encoded_len_u64(value as u64)
}

/// Encoded length of `value` as a `u32` varint, in bytes (1 to 5).
///
/// # Example
///
/// ```
/// use wire_codec::varint;
/// assert_eq!(varint::encoded_len_u32(0), 1);
/// assert_eq!(varint::encoded_len_u32(127), 1);
/// assert_eq!(varint::encoded_len_u32(128), 2);
/// assert_eq!(varint::encoded_len_u32(u32::MAX), 5);
/// ```
#[inline]
#[must_use]
pub const fn encoded_len_u32(value: u32) -> usize {
    encoded_len_u64(value as u64)
}

/// Encoded length of `value` as a `u64` varint, in bytes (1 to 10).
///
/// # Example
///
/// ```
/// use wire_codec::varint;
/// assert_eq!(varint::encoded_len_u64(0), 1);
/// assert_eq!(varint::encoded_len_u64(u64::MAX), 10);
/// ```
#[inline]
#[must_use]
pub const fn encoded_len_u64(value: u64) -> usize {
    let bits = (u64::BITS - value.leading_zeros()) as usize;
    if bits == 0 { 1 } else { bits.div_ceil(7) }
}

/// Encoded length of `value` as a `usize` varint, in bytes (1 to 10).
///
/// # Example
///
/// ```
/// use wire_codec::varint;
/// assert_eq!(varint::encoded_len_usize(300), 2);
/// ```
#[inline]
#[must_use]
pub const fn encoded_len_usize(value: usize) -> usize {
    // `usize` is at most 64 bits on every supported target.
    encoded_len_u64(value as u64)
}

/// Encoded length of `value` as a zigzag `i32` varint, in bytes (1 to 5).
///
/// # Example
///
/// ```
/// use wire_codec::varint;
/// assert_eq!(varint::encoded_len_i32(-1), 1);
/// assert_eq!(varint::encoded_len_i32(i32::MIN), 5);
/// ```
#[inline]
#[must_use]
pub const fn encoded_len_i32(value: i32) -> usize {
    encoded_len_u32(zigzag::encode_i32(value))
}

/// Encoded length of `value` as a zigzag `i64` varint, in bytes (1 to 10).
///
/// # Example
///
/// ```
/// use wire_codec::varint;
/// assert_eq!(varint::encoded_len_i64(-750), 2);
/// assert_eq!(varint::encoded_len_i64(i64::MIN), 10);
/// ```
#[inline]
#[must_use]
pub const fn encoded_len_i64(value: i64) -> usize {
    encoded_len_u64(zigzag::encode_i64(value))
}

/// Encode `value` as a `u16` varint into `buf`.
///
/// # Errors
///
/// Returns [`Error::BufferFull`] if `buf` has fewer than [`encoded_len_u16`]
/// bytes available. Nothing is written on error.
///
/// # Example
///
/// ```
/// use wire_codec::{varint, WriteBuf};
/// let mut out = [0u8; 3];
/// let mut buf = WriteBuf::new(&mut out);
/// varint::encode_u16(300, &mut buf).unwrap();
/// assert_eq!(buf.written(), &[0xAC, 0x02]);
/// ```
#[inline]
pub fn encode_u16(value: u16, buf: &mut WriteBuf<'_>) -> Result<()> {
    encode_raw(u64::from(value), buf)
}

/// Encode `value` as a `u32` varint into `buf`.
///
/// # Errors
///
/// Returns [`Error::BufferFull`] if `buf` has fewer than [`encoded_len_u32`]
/// bytes available. Nothing is written on error.
///
/// # Example
///
/// ```
/// use wire_codec::{varint, WriteBuf};
/// let mut out = [0u8; 5];
/// let mut buf = WriteBuf::new(&mut out);
/// varint::encode_u32(300, &mut buf).unwrap();
/// assert_eq!(buf.written(), &[0xAC, 0x02]);
/// ```
#[inline]
pub fn encode_u32(value: u32, buf: &mut WriteBuf<'_>) -> Result<()> {
    encode_raw(u64::from(value), buf)
}

/// Encode `value` as a `u64` varint into `buf`.
///
/// # Errors
///
/// Returns [`Error::BufferFull`] if `buf` has fewer than [`encoded_len_u64`]
/// bytes available. Nothing is written on error.
///
/// # Example
///
/// ```
/// use wire_codec::{varint, WriteBuf};
/// let mut out = [0u8; varint::MAX_LEN_U64];
/// let mut buf = WriteBuf::new(&mut out);
/// varint::encode_u64(u64::MAX, &mut buf).unwrap();
/// assert_eq!(buf.position(), 10);
/// ```
#[inline]
pub fn encode_u64(value: u64, buf: &mut WriteBuf<'_>) -> Result<()> {
    encode_raw(value, buf)
}

/// Encode `value` as a `usize` varint into `buf`.
///
/// The wire format is identical to [`encode_u64`], so a value written on a
/// 64-bit host can be read by [`decode_usize`] on a 32-bit host whenever it
/// fits.
///
/// # Errors
///
/// Returns [`Error::BufferFull`] if `buf` has fewer than
/// [`encoded_len_usize`] bytes available. Nothing is written on error.
///
/// # Example
///
/// ```
/// use wire_codec::{varint, WriteBuf};
/// let payload = b"hello";
/// let mut out = [0u8; 16];
/// let mut buf = WriteBuf::new(&mut out);
/// varint::encode_usize(payload.len(), &mut buf).unwrap();
/// assert_eq!(buf.written(), &[5]);
/// ```
#[inline]
pub fn encode_usize(value: usize, buf: &mut WriteBuf<'_>) -> Result<()> {
    // `usize` is at most 64 bits on every supported target.
    encode_raw(value as u64, buf)
}

/// Encode `value` as a zigzag `i32` varint (protobuf `sint32`) into `buf`.
///
/// # Errors
///
/// Returns [`Error::BufferFull`] if `buf` has fewer than [`encoded_len_i32`]
/// bytes available. Nothing is written on error.
///
/// # Example
///
/// ```
/// use wire_codec::{varint, WriteBuf};
/// let mut out = [0u8; 5];
/// let mut buf = WriteBuf::new(&mut out);
/// varint::encode_i32(-2, &mut buf).unwrap();
/// assert_eq!(buf.written(), &[3]);
/// ```
#[inline]
pub fn encode_i32(value: i32, buf: &mut WriteBuf<'_>) -> Result<()> {
    encode_raw(u64::from(zigzag::encode_i32(value)), buf)
}

/// Encode `value` as a zigzag `i64` varint (protobuf `sint64`) into `buf`.
///
/// # Errors
///
/// Returns [`Error::BufferFull`] if `buf` has fewer than [`encoded_len_i64`]
/// bytes available. Nothing is written on error.
///
/// # Example
///
/// ```
/// use wire_codec::{varint, WriteBuf};
/// let mut out = [0u8; 10];
/// let mut buf = WriteBuf::new(&mut out);
/// varint::encode_i64(-750, &mut buf).unwrap();
/// assert_eq!(buf.written(), &[0xDB, 0x0B]);
/// ```
#[inline]
pub fn encode_i64(value: i64, buf: &mut WriteBuf<'_>) -> Result<()> {
    encode_raw(zigzag::encode_i64(value), buf)
}

/// Decode a `u16` varint from `buf`.
///
/// # Errors
///
/// - [`Error::UnexpectedEof`] if `buf` is exhausted before the terminator byte.
/// - [`Error::VarintOverflow`] if the decoded value exceeds `u16::MAX` or if
///   more than [`MAX_LEN_U16`] bytes would be consumed.
///
/// The cursor is unchanged on error.
///
/// # Example
///
/// ```
/// use wire_codec::{varint, ReadBuf};
/// let mut buf = ReadBuf::new(&[0xFF, 0xFF, 0x03]);
/// assert_eq!(varint::decode_u16(&mut buf).unwrap(), u16::MAX);
/// ```
#[inline]
pub fn decode_u16(buf: &mut ReadBuf<'_>) -> Result<u16> {
    // The width check inside `decode_with` guarantees the value fits.
    decode_with(buf, MAX_LEN_U16, u16::BITS).map(|v| v as u16)
}

/// Decode a `u32` varint from `buf`.
///
/// # Errors
///
/// - [`Error::UnexpectedEof`] if `buf` is exhausted before the terminator byte.
/// - [`Error::VarintOverflow`] if the decoded value exceeds `u32::MAX` or if
///   more than [`MAX_LEN_U32`] bytes would be consumed.
///
/// The cursor is unchanged on error.
///
/// # Example
///
/// ```
/// use wire_codec::{varint, ReadBuf};
/// let mut buf = ReadBuf::new(&[0xAC, 0x02]);
/// assert_eq!(varint::decode_u32(&mut buf).unwrap(), 300);
/// ```
#[inline]
pub fn decode_u32(buf: &mut ReadBuf<'_>) -> Result<u32> {
    decode_with(buf, MAX_LEN_U32, u32::BITS).map(|v| v as u32)
}

/// Decode a `u64` varint from `buf`.
///
/// # Errors
///
/// - [`Error::UnexpectedEof`] if `buf` is exhausted before the terminator byte.
/// - [`Error::VarintOverflow`] if more than [`MAX_LEN_U64`] bytes would be
///   consumed or the final byte carries bits that would overflow `u64`.
///
/// The cursor is unchanged on error.
///
/// # Example
///
/// ```
/// use wire_codec::{varint, ReadBuf};
/// let bytes = [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x01];
/// let mut buf = ReadBuf::new(&bytes);
/// assert_eq!(varint::decode_u64(&mut buf).unwrap(), u64::MAX);
/// assert!(buf.is_empty());
/// ```
#[inline]
pub fn decode_u64(buf: &mut ReadBuf<'_>) -> Result<u64> {
    decode_with(buf, MAX_LEN_U64, u64::BITS)
}

/// Decode a `usize` varint from `buf`.
///
/// The bytes are parsed as a `u64` varint and then converted, so the
/// accepted wire format is the same on every target.
///
/// # Errors
///
/// - [`Error::UnexpectedEof`] if `buf` is exhausted before the terminator byte.
/// - [`Error::VarintOverflow`] if the value overflows `u64`, or does not fit
///   in `usize` on this target (values above `u32::MAX` on 32-bit hosts).
///
/// The cursor is unchanged on error.
///
/// # Example
///
/// ```
/// use wire_codec::{varint, ReadBuf};
/// let mut buf = ReadBuf::new(&[0x05, b'h', b'e', b'l', b'l', b'o']);
/// let len = varint::decode_usize(&mut buf).unwrap();
/// assert_eq!(buf.read_bytes(len).unwrap(), b"hello");
/// ```
#[inline]
pub fn decode_usize(buf: &mut ReadBuf<'_>) -> Result<usize> {
    decode_usize_with(buf)
}

/// Decode a zigzag `i32` varint (protobuf `sint32`) from `buf`.
///
/// # Errors
///
/// Same contract as [`decode_u32`]. The cursor is unchanged on error.
///
/// # Example
///
/// ```
/// use wire_codec::{varint, ReadBuf};
/// let mut buf = ReadBuf::new(&[0x03]);
/// assert_eq!(varint::decode_i32(&mut buf).unwrap(), -2);
/// ```
#[inline]
pub fn decode_i32(buf: &mut ReadBuf<'_>) -> Result<i32> {
    decode_u32(buf).map(zigzag::decode_i32)
}

/// Decode a zigzag `i64` varint (protobuf `sint64`) from `buf`.
///
/// # Errors
///
/// Same contract as [`decode_u64`]. The cursor is unchanged on error.
///
/// # Example
///
/// ```
/// use wire_codec::{varint, ReadBuf};
/// let mut buf = ReadBuf::new(&[0xDB, 0x0B]);
/// assert_eq!(varint::decode_i64(&mut buf).unwrap(), -750);
/// ```
#[inline]
pub fn decode_i64(buf: &mut ReadBuf<'_>) -> Result<i64> {
    decode_u64(buf).map(zigzag::decode_i64)
}

/// Encode `value` straight into the unwritten tail of `buf`.
///
/// The length is computed up front, so there is a single capacity check and
/// nothing is written when it fails.
#[inline(always)]
fn encode_raw(mut value: u64, buf: &mut WriteBuf<'_>) -> Result<()> {
    let len = encoded_len_u64(value);
    let Some(dst) = buf.spare_mut().get_mut(..len) else {
        return Err(Error::BufferFull);
    };
    // `encoded_len_u64` is always in `1..=10`, so `last` is in bounds.
    let last = len - 1;
    for byte in &mut dst[..last] {
        // Truncation to the low 7 bits is the encoding itself.
        *byte = (value as u8) | 0x80;
        value >>= 7;
    }
    dst[last] = value as u8;
    buf.commit(len);
    Ok(())
}

/// Decode from `buf`, committing the cursor only on success.
#[inline(always)]
fn decode_with(buf: &mut ReadBuf<'_>, max_len: usize, bits: u32) -> Result<u64> {
    let (value, len) = decode_slice(buf.remaining_slice(), max_len, bits)?;
    buf.consume(len);
    Ok(value)
}

#[inline(always)]
fn decode_usize_with(buf: &mut ReadBuf<'_>) -> Result<usize> {
    let (value, len) = decode_slice(buf.remaining_slice(), MAX_LEN_U64, u64::BITS)?;
    let value = usize::try_from(value).map_err(|_| Error::VarintOverflow)?;
    buf.consume(len);
    Ok(value)
}

/// Core LEB128 decoder over a plain slice.
///
/// Returns the decoded value and the number of bytes it occupied. `bits` is
/// the width of the target type; `max_len` is `ceil(bits / 7)`. The final
/// byte of a maximum-length encoding may only carry `bits - 7 * (max_len - 1)`
/// payload bits; anything above that overflows the target.
#[inline(always)]
fn decode_slice(bytes: &[u8], max_len: usize, bits: u32) -> Result<(u64, usize)> {
    let mut value: u64 = 0;
    let mut shift: u32 = 0;
    for i in 0..max_len {
        let Some(&byte) = bytes.get(i) else {
            return Err(Error::UnexpectedEof);
        };
        value |= u64::from(byte & 0x7F) << shift;
        if byte & 0x80 == 0 {
            if i == max_len - 1 && u32::from(byte) >> (bits - shift) != 0 {
                return Err(Error::VarintOverflow);
            }
            return Ok((value, i + 1));
        }
        shift += 7;
    }
    Err(Error::VarintOverflow)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip_u32(value: u32) {
        let mut storage = [0u8; MAX_LEN_U32];
        let mut w = WriteBuf::new(&mut storage);
        encode_u32(value, &mut w).unwrap();
        let n = w.position();
        assert_eq!(n, encoded_len_u32(value));
        let mut r = ReadBuf::new(&storage[..n]);
        assert_eq!(decode_u32(&mut r).unwrap(), value);
        assert!(r.is_empty());
    }

    fn round_trip_u64(value: u64) {
        let mut storage = [0u8; MAX_LEN_U64];
        let mut w = WriteBuf::new(&mut storage);
        encode_u64(value, &mut w).unwrap();
        let n = w.position();
        assert_eq!(n, encoded_len_u64(value));
        let mut r = ReadBuf::new(&storage[..n]);
        assert_eq!(decode_u64(&mut r).unwrap(), value);
        assert!(r.is_empty());
    }

    #[test]
    fn u32_boundaries() {
        for &v in &[
            0u32,
            1,
            127,
            128,
            16_383,
            16_384,
            2_097_151,
            2_097_152,
            u32::MAX,
        ] {
            round_trip_u32(v);
        }
    }

    #[test]
    fn u64_boundaries() {
        for &v in &[
            0u64,
            127,
            128,
            1 << 35,
            1 << 56,
            (1 << 63) - 1,
            1 << 63,
            u64::MAX,
        ] {
            round_trip_u64(v);
        }
    }

    #[test]
    fn u16_round_trip_and_bounds() {
        for v in [0u16, 1, 127, 128, 16_383, 16_384, u16::MAX] {
            let mut storage = [0u8; MAX_LEN_U16];
            let mut w = WriteBuf::new(&mut storage);
            encode_u16(v, &mut w).unwrap();
            let n = w.position();
            assert_eq!(n, encoded_len_u16(v));
            let mut r = ReadBuf::new(&storage[..n]);
            assert_eq!(decode_u16(&mut r).unwrap(), v);
        }
        // 0x04 in the third byte is bit 16: one past u16::MAX.
        let mut r = ReadBuf::new(&[0xFF, 0xFF, 0x04]);
        assert_eq!(decode_u16(&mut r), Err(Error::VarintOverflow));
        let mut r = ReadBuf::new(&[0x80, 0x80, 0x80, 0x00]);
        assert_eq!(decode_u16(&mut r), Err(Error::VarintOverflow));
    }

    #[test]
    fn signed_round_trip() {
        for v in [0i32, 1, -1, 63, -64, 64, -65, i32::MAX, i32::MIN] {
            let mut storage = [0u8; MAX_LEN_U32];
            let mut w = WriteBuf::new(&mut storage);
            encode_i32(v, &mut w).unwrap();
            let n = w.position();
            assert_eq!(n, encoded_len_i32(v));
            assert_eq!(decode_i32(&mut ReadBuf::new(&storage[..n])).unwrap(), v);
        }
        for v in [0i64, 1, -1, i64::from(i32::MIN) - 1, i64::MAX, i64::MIN] {
            let mut storage = [0u8; MAX_LEN_U64];
            let mut w = WriteBuf::new(&mut storage);
            encode_i64(v, &mut w).unwrap();
            let n = w.position();
            assert_eq!(n, encoded_len_i64(v));
            assert_eq!(decode_i64(&mut ReadBuf::new(&storage[..n])).unwrap(), v);
        }
    }

    #[test]
    fn usize_round_trip() {
        for v in [0usize, 1, 300, usize::MAX] {
            let mut storage = [0u8; MAX_LEN_U64];
            let mut w = WriteBuf::new(&mut storage);
            encode_usize(v, &mut w).unwrap();
            let n = w.position();
            assert_eq!(n, encoded_len_usize(v));
            assert_eq!(decode_usize(&mut ReadBuf::new(&storage[..n])).unwrap(), v);
        }
    }

    #[test]
    fn usize_too_wide_for_target_overflows() {
        // 2^32 encoded as a u64 varint.
        let bytes = [0x80, 0x80, 0x80, 0x80, 0x10];
        let mut r = ReadBuf::new(&bytes);
        let result = decode_usize(&mut r);
        if usize::BITS > 32 {
            assert_eq!(result.map(|v| v as u64), Ok(1u64 << 32));
        } else {
            assert_eq!(result, Err(Error::VarintOverflow));
            assert_eq!(r.position(), 0);
        }
    }

    #[test]
    fn decode_u32_overflow_on_sixth_byte() {
        let mut buf = ReadBuf::new(&[0x80, 0x80, 0x80, 0x80, 0x80, 0x01]);
        assert_eq!(decode_u32(&mut buf), Err(Error::VarintOverflow));
        assert_eq!(buf.position(), 0, "cursor is unchanged on error");
    }

    #[test]
    fn decode_u32_overflow_in_final_byte() {
        // Five bytes, terminator carries bits above the 32-bit window.
        for last in [0x10u8, 0x1F, 0x7F] {
            let bytes = [0xFF, 0xFF, 0xFF, 0xFF, last];
            let mut buf = ReadBuf::new(&bytes);
            assert_eq!(decode_u32(&mut buf), Err(Error::VarintOverflow));
        }
        let mut buf = ReadBuf::new(&[0xFF, 0xFF, 0xFF, 0xFF, 0x0F]);
        assert_eq!(decode_u32(&mut buf), Ok(u32::MAX));
    }

    #[test]
    fn decode_u64_final_byte_rules() {
        let mut bytes = [0xFFu8; MAX_LEN_U64];
        for last in [0x02u8, 0x03, 0x7F, 0x81, 0x80] {
            bytes[9] = last;
            let mut buf = ReadBuf::new(&bytes);
            assert_eq!(
                decode_u64(&mut buf),
                Err(Error::VarintOverflow),
                "last={last:#x}"
            );
            assert_eq!(buf.position(), 0);
        }
        bytes[9] = 0x01;
        assert_eq!(decode_u64(&mut ReadBuf::new(&bytes)), Ok(u64::MAX));
        bytes[9] = 0x00;
        // An overlong (padded) form is accepted.
        assert_eq!(decode_u64(&mut ReadBuf::new(&bytes)), Ok(u64::MAX >> 1));
    }

    #[test]
    fn decode_truncated_leaves_cursor_unchanged() {
        // Continuation bit set on every byte present.
        let mut buf = ReadBuf::new(&[0x80, 0x80]);
        assert_eq!(decode_u32(&mut buf), Err(Error::UnexpectedEof));
        assert_eq!(buf.position(), 0);
        let mut buf = ReadBuf::new(&[]);
        assert_eq!(decode_u64(&mut buf), Err(Error::UnexpectedEof));
    }

    #[test]
    fn streaming_retry_after_eof_succeeds() {
        let wire = [0xAC, 0x02, 0x07];
        let mut filled = 0;
        let value = loop {
            let mut r = ReadBuf::new(&wire[..filled]);
            match decode_u32(&mut r) {
                Ok(v) => {
                    assert_eq!(r.position(), 2);
                    break v;
                }
                Err(Error::UnexpectedEof) => {
                    assert_eq!(r.position(), 0);
                    filled += 1;
                }
                Err(e) => panic!("unexpected {e}"),
            }
        };
        assert_eq!(value, 300);
        assert_eq!(filled, 2);
    }

    #[test]
    fn encode_is_atomic_on_buffer_full() {
        let mut storage = [0xEEu8; 1];
        let mut w = WriteBuf::new(&mut storage);
        assert_eq!(encode_u32(300, &mut w), Err(Error::BufferFull));
        assert_eq!(w.position(), 0);
        assert_eq!(storage, [0xEE], "nothing is written on error");

        let mut storage = [0u8; 9];
        let mut w = WriteBuf::new(&mut storage);
        assert_eq!(encode_u64(u64::MAX, &mut w), Err(Error::BufferFull));
        assert_eq!(w.position(), 0);

        let mut storage = [0u8; 0];
        let mut w = WriteBuf::new(&mut storage);
        assert_eq!(encode_u16(0, &mut w), Err(Error::BufferFull));
        assert_eq!(encode_i32(0, &mut w), Err(Error::BufferFull));
        assert_eq!(encode_i64(0, &mut w), Err(Error::BufferFull));
        assert_eq!(encode_usize(0, &mut w), Err(Error::BufferFull));
    }

    #[test]
    fn exact_wire_bytes() {
        let mut storage = [0u8; MAX_LEN_U64];
        let mut w = WriteBuf::new(&mut storage);
        encode_u64(0x80, &mut w).unwrap();
        assert_eq!(w.written(), &[0x80, 0x01]);
        let mut w = WriteBuf::new(&mut storage);
        encode_u32(u32::MAX, &mut w).unwrap();
        assert_eq!(w.written(), &[0xFF, 0xFF, 0xFF, 0xFF, 0x0F]);
    }
}
