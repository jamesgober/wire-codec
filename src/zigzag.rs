//! Zigzag encoding for signed integers.
//!
//! Zigzag maps signed integers to unsigned ones so that small-magnitude negative
//! values produce small-magnitude varints. Pair these helpers with the
//! [`varint`][`crate::varint`] functions to encode signed values compactly.
//!
//! The mapping mirrors the one used by Protocol Buffers:
//!
//! | signed | unsigned |
//! | ------ | -------- |
//! |  0     | 0        |
//! | -1     | 1        |
//! |  1     | 2        |
//! | -2     | 3        |
//! |  2     | 4        |

/// Map a signed `i32` to its zigzag-encoded `u32`.
///
/// # Example
///
/// ```
/// use wire_codec::zigzag;
/// assert_eq!(zigzag::encode_i32(0), 0);
/// assert_eq!(zigzag::encode_i32(-1), 1);
/// assert_eq!(zigzag::encode_i32(1), 2);
/// assert_eq!(zigzag::encode_i32(i32::MIN), u32::MAX);
/// ```
#[inline]
pub const fn encode_i32(value: i32) -> u32 {
    ((value << 1) ^ (value >> 31)) as u32
}

/// Recover the signed `i32` from a zigzag-encoded `u32`.
///
/// # Example
///
/// ```
/// use wire_codec::zigzag;
/// assert_eq!(zigzag::decode_i32(0), 0);
/// assert_eq!(zigzag::decode_i32(1), -1);
/// assert_eq!(zigzag::decode_i32(u32::MAX), i32::MIN);
/// ```
#[inline]
pub const fn decode_i32(value: u32) -> i32 {
    ((value >> 1) as i32) ^ -((value & 1) as i32)
}

/// Map a signed `i64` to its zigzag-encoded `u64`.
///
/// # Example
///
/// ```
/// use wire_codec::zigzag;
/// assert_eq!(zigzag::encode_i64(i64::MIN), u64::MAX);
/// ```
#[inline]
pub const fn encode_i64(value: i64) -> u64 {
    ((value << 1) ^ (value >> 63)) as u64
}

/// Recover the signed `i64` from a zigzag-encoded `u64`.
#[inline]
pub const fn decode_i64(value: u64) -> i64 {
    ((value >> 1) as i64) ^ -((value & 1) as i64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn i32_round_trip() {
        for v in [0i32, 1, -1, 2, -2, 100, -100, i32::MAX, i32::MIN] {
            assert_eq!(decode_i32(encode_i32(v)), v);
        }
    }

    #[test]
    fn i64_round_trip() {
        for v in [0i64, 1, -1, i64::MAX, i64::MIN, 1 << 40, -(1 << 40)] {
            assert_eq!(decode_i64(encode_i64(v)), v);
        }
    }
}
