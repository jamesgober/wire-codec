//! Unsigned LEB128 varint encoding.
//!
//! A varint encodes an unsigned integer in one to ten bytes. The low seven
//! bits of each byte carry payload, and the top bit signals continuation:
//! `1` means more bytes follow, `0` means the value is complete.
//!
//! The encoding is identical to the format used by Protocol Buffers, WebAssembly,
//! and DWARF.

use crate::buf::{ReadBuf, WriteBuf};
use crate::error::{Error, Result};

/// Maximum encoded length of a `u16` varint.
pub const MAX_LEN_U16: usize = 3;
/// Maximum encoded length of a `u32` varint.
pub const MAX_LEN_U32: usize = 5;
/// Maximum encoded length of a `u64` varint.
pub const MAX_LEN_U64: usize = 10;

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
// Manual ceiling-divide. `usize::div_ceil` is not const-stable on MSRV 1.75.
// `unknown_lints` shields older clippy releases that lack `manual_div_ceil`.
#[allow(unknown_lints, clippy::manual_div_ceil)]
pub const fn encoded_len_u32(value: u32) -> usize {
    let bits = (u32::BITS - value.leading_zeros()) as usize;
    if bits == 0 {
        1
    } else {
        (bits + 6) / 7
    }
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
// Manual ceiling-divide. `usize::div_ceil` is not const-stable on MSRV 1.75.
// `unknown_lints` shields older clippy releases that lack `manual_div_ceil`.
#[allow(unknown_lints, clippy::manual_div_ceil)]
pub const fn encoded_len_u64(value: u64) -> usize {
    let bits = (u64::BITS - value.leading_zeros()) as usize;
    if bits == 0 {
        1
    } else {
        (bits + 6) / 7
    }
}

/// Encode `value` as a `u32` varint into `buf`.
///
/// # Errors
///
/// Returns [`Error::BufferFull`] if `buf` has fewer than [`encoded_len_u32`]
/// bytes available.
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
pub fn encode_u32(mut value: u32, buf: &mut WriteBuf<'_>) -> Result<()> {
    while value >= 0x80 {
        buf.write_u8(((value & 0x7F) as u8) | 0x80)?;
        value >>= 7;
    }
    buf.write_u8(value as u8)
}

/// Encode `value` as a `u64` varint into `buf`.
///
/// # Errors
///
/// Returns [`Error::BufferFull`] if `buf` has fewer than [`encoded_len_u64`]
/// bytes available.
#[inline]
pub fn encode_u64(mut value: u64, buf: &mut WriteBuf<'_>) -> Result<()> {
    while value >= 0x80 {
        buf.write_u8(((value & 0x7F) as u8) | 0x80)?;
        value >>= 7;
    }
    buf.write_u8(value as u8)
}

/// Decode a `u32` varint from `buf`.
///
/// # Errors
///
/// - [`Error::UnexpectedEof`] if `buf` is exhausted before the terminator byte.
/// - [`Error::VarintOverflow`] if the decoded value exceeds `u32::MAX` or if
///   more than [`MAX_LEN_U32`] bytes are consumed.
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
    let mut value: u64 = 0;
    let mut shift: u32 = 0;
    for i in 0..MAX_LEN_U32 {
        let byte = buf.read_u8()?;
        value |= u64::from(byte & 0x7F) << shift;
        if byte & 0x80 == 0 {
            if value > u64::from(u32::MAX) {
                return Err(Error::VarintOverflow);
            }
            // Final byte of a 5-byte u32 varint can only hold 4 payload bits.
            if i == MAX_LEN_U32 - 1 && byte > 0x0F {
                return Err(Error::VarintOverflow);
            }
            return Ok(value as u32);
        }
        shift += 7;
    }
    Err(Error::VarintOverflow)
}

/// Decode a `u64` varint from `buf`.
///
/// # Errors
///
/// - [`Error::UnexpectedEof`] if `buf` is exhausted before the terminator byte.
/// - [`Error::VarintOverflow`] if more than [`MAX_LEN_U64`] bytes are consumed
///   or the final byte carries bits that would overflow `u64`.
#[inline]
pub fn decode_u64(buf: &mut ReadBuf<'_>) -> Result<u64> {
    let mut value: u64 = 0;
    let mut shift: u32 = 0;
    for i in 0..MAX_LEN_U64 {
        let byte = buf.read_u8()?;
        if i == MAX_LEN_U64 - 1 {
            // Final byte of a 10-byte u64 varint can only carry 1 payload bit.
            if byte & 0xFE != 0 {
                return Err(Error::VarintOverflow);
            }
            value |= u64::from(byte & 0x01) << shift;
            return Ok(value);
        }
        value |= u64::from(byte & 0x7F) << shift;
        if byte & 0x80 == 0 {
            return Ok(value);
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
        for &v in &[0u64, 127, 128, 1 << 35, 1 << 56, u64::MAX] {
            round_trip_u64(v);
        }
    }

    #[test]
    fn decode_u32_overflow_on_sixth_byte() {
        let mut buf = ReadBuf::new(&[0x80, 0x80, 0x80, 0x80, 0x80, 0x01]);
        assert_eq!(decode_u32(&mut buf), Err(Error::VarintOverflow));
    }

    #[test]
    fn decode_u32_overflow_in_final_byte() {
        // Five bytes, terminator carries bits above the 32-bit window.
        let mut buf = ReadBuf::new(&[0xFF, 0xFF, 0xFF, 0xFF, 0x10]);
        assert_eq!(decode_u32(&mut buf), Err(Error::VarintOverflow));
    }

    #[test]
    fn decode_truncated() {
        // Continuation bit set on the only byte present.
        let mut buf = ReadBuf::new(&[0x80]);
        assert_eq!(decode_u32(&mut buf), Err(Error::UnexpectedEof));
    }
}
