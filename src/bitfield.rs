//! Bit-level read and write cursors.
//!
//! [`BitReader`] consumes bits from a borrowed byte slice in most-significant-bit
//! first order, suitable for packed binary protocols where multiple fields share
//! a single byte. [`BitWriter`] performs the reverse operation against a caller-
//! supplied output buffer.
//!
//! Both cursors support arbitrary widths from 1 to 64 bits per call.

use crate::error::{Error, Result};

/// Maximum bit width per `read_bits` / `write_bits` call.
pub const MAX_BIT_WIDTH: u32 = 64;

/// Reads packed bits from a borrowed byte slice, most-significant-bit first.
///
/// # Example
///
/// ```
/// use wire_codec::BitReader;
///
/// // 0b1010_1100 = 0xAC. Read 3 bits, then 5 bits.
/// let mut r = BitReader::new(&[0xAC]);
/// assert_eq!(r.read_bits(3).unwrap(), 0b101);
/// assert_eq!(r.read_bits(5).unwrap(), 0b01100);
/// ```
#[derive(Debug, Clone)]
pub struct BitReader<'a> {
    bytes: &'a [u8],
    bit_pos: usize,
}

impl<'a> BitReader<'a> {
    /// Wrap `bytes` in a new bit cursor positioned at the first bit.
    #[inline]
    pub const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, bit_pos: 0 }
    }

    /// Number of bits already consumed.
    #[inline]
    pub const fn bits_consumed(&self) -> usize {
        self.bit_pos
    }

    /// Number of bits still available to read.
    #[inline]
    pub const fn bits_remaining(&self) -> usize {
        bit_capacity(self.bytes.len()) - self.bit_pos
    }

    /// Advance the cursor to the next byte boundary.
    ///
    /// Has no effect when the cursor is already byte-aligned. The skipped
    /// bits are discarded without being inspected.
    #[inline]
    pub fn align_to_byte(&mut self) {
        // `bit_pos <= bit_capacity(..)`, which is a multiple of 8 no larger
        // than `usize::MAX - 7`, so the addition cannot overflow and the
        // result never passes the end of the input.
        self.bit_pos = (self.bit_pos + 7) & !7;
    }

    /// Read the next `n` bits as the low-order bits of a `u64`.
    ///
    /// # Errors
    ///
    /// - [`Error::BitOverflow`] if `n` is `0` or greater than [`MAX_BIT_WIDTH`].
    /// - [`Error::UnexpectedEof`] if fewer than `n` bits remain in the input.
    pub fn read_bits(&mut self, n: u32) -> Result<u64> {
        if n == 0 || n > MAX_BIT_WIDTH {
            return Err(Error::BitOverflow);
        }
        if (n as usize) > self.bits_remaining() {
            return Err(Error::UnexpectedEof);
        }
        let mut value: u64 = 0;
        let mut bits_left = n;
        while bits_left > 0 {
            let byte_idx = self.bit_pos / 8;
            let bit_off = (self.bit_pos % 8) as u32;
            let avail = 8 - bit_off;
            let take = if bits_left < avail { bits_left } else { avail };
            let shift = avail - take;
            let mask: u32 = (1u32 << take) - 1;
            let chunk = (u32::from(self.bytes[byte_idx]) >> shift) & mask;
            value = (value << take) | u64::from(chunk);
            self.bit_pos += take as usize;
            bits_left -= take;
        }
        Ok(value)
    }

    /// Read one bit as a `bool`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnexpectedEof`] if no bits remain.
    ///
    /// # Example
    ///
    /// ```
    /// use wire_codec::BitReader;
    ///
    /// let mut r = BitReader::new(&[0b1000_0000]);
    /// assert!(r.read_bool().unwrap());
    /// assert!(!r.read_bool().unwrap());
    /// ```
    #[inline]
    pub fn read_bool(&mut self) -> Result<bool> {
        self.read_bits(1).map(|bit| bit != 0)
    }

    /// Read the next `n` bits as a two's-complement signed value, sign-extended
    /// to `i64`.
    ///
    /// # Errors
    ///
    /// Same contract as [`BitReader::read_bits`].
    ///
    /// # Example
    ///
    /// ```
    /// use wire_codec::BitReader;
    ///
    /// // 0b101 in a 3-bit signed field is -3.
    /// let mut r = BitReader::new(&[0b1010_0000]);
    /// assert_eq!(r.read_signed_bits(3).unwrap(), -3);
    /// ```
    #[inline]
    pub fn read_signed_bits(&mut self, n: u32) -> Result<i64> {
        self.read_bits(n).map(|raw| sign_extend(raw, n))
    }
}

/// Interpret the low `n` bits of `raw` (1 <= n <= 64) as two's complement.
#[inline]
pub(crate) const fn sign_extend(raw: u64, n: u32) -> i64 {
    let shift = 64 - n;
    ((raw << shift) as i64) >> shift
}

/// Encode `value` as an `n`-bit two's-complement field, or `None` if it does
/// not fit.
#[inline]
pub(crate) fn signed_to_bits(value: i64, n: u32) -> Option<u64> {
    if n == 0 || n > MAX_BIT_WIDTH {
        return None;
    }
    let shift = 64 - n;
    // The value fits iff truncating to `n` bits and sign-extending back
    // reproduces it.
    let raw = (value as u64) & (u64::MAX >> shift);
    (sign_extend(raw, n) == value).then_some(raw)
}

/// Writes packed bits to a caller-supplied byte slice, most-significant-bit first.
///
/// Every bit inside the prefix reported by [`BitWriter::finish`] is written
/// explicitly: field bits by [`BitWriter::write_bits`], padding bits as zero
/// by [`BitWriter::align_to_byte`] and [`BitWriter::finish`]. A reused buffer
/// holding stale data therefore produces the same output as a zeroed one.
/// Bytes past that prefix are left untouched.
///
/// # Example
///
/// ```
/// use wire_codec::BitWriter;
///
/// let mut storage = [0u8; 1];
/// let mut w = BitWriter::new(&mut storage);
/// w.write_bits(0b101, 3).unwrap();
/// w.write_bits(0b01100, 5).unwrap();
/// assert_eq!(w.finish(), 1);
/// assert_eq!(storage[0], 0xAC);
/// ```
#[derive(Debug)]
pub struct BitWriter<'a> {
    bytes: &'a mut [u8],
    bit_pos: usize,
}

impl<'a> BitWriter<'a> {
    /// Wrap `bytes` in a new bit cursor. Existing contents of `bytes` are
    /// overwritten as bits are written.
    #[inline]
    pub fn new(bytes: &'a mut [u8]) -> Self {
        Self { bytes, bit_pos: 0 }
    }

    /// Number of bits written so far.
    #[inline]
    pub fn bits_written(&self) -> usize {
        self.bit_pos
    }

    /// Pad the cursor with zero bits up to the next byte boundary.
    ///
    /// Has no effect when the cursor is already byte-aligned.
    ///
    /// # Errors
    ///
    /// Never fails in practice: a partially written byte always lies inside
    /// the backing slice, so rounding up to its end cannot run out of room.
    /// The `Result` return type is kept for API stability.
    pub fn align_to_byte(&mut self) -> Result<()> {
        self.zero_trailing_bits();
        // `bit_pos <= bit_capacity(..)`, a multiple of 8 no larger than
        // `usize::MAX - 7`: the addition cannot overflow and the rounded
        // position cannot pass the end of the slice.
        self.bit_pos = (self.bit_pos + 7) & !7;
        Ok(())
    }

    /// Clear the unwritten low-order bits of a partially written byte.
    #[inline]
    fn zero_trailing_bits(&mut self) {
        let bit_off = self.bit_pos % 8;
        if bit_off != 0 {
            let keep = 0xFFu8 << (8 - bit_off);
            self.bytes[self.bit_pos / 8] &= keep;
        }
    }

    /// Write the low `n` bits of `value` into the buffer.
    ///
    /// # Errors
    ///
    /// - [`Error::BitOverflow`] if `n` is `0`, greater than [`MAX_BIT_WIDTH`],
    ///   or `value` has bits set above position `n`.
    /// - [`Error::BufferFull`] if fewer than `n` bits remain in the buffer.
    pub fn write_bits(&mut self, value: u64, n: u32) -> Result<()> {
        if n == 0 || n > MAX_BIT_WIDTH {
            return Err(Error::BitOverflow);
        }
        if n < 64 && (value >> n) != 0 {
            return Err(Error::BitOverflow);
        }
        if (n as usize) > bit_capacity(self.bytes.len()) - self.bit_pos {
            return Err(Error::BufferFull);
        }
        let mut bits_left = n;
        while bits_left > 0 {
            let byte_idx = self.bit_pos / 8;
            let bit_off = (self.bit_pos % 8) as u32;
            let avail = 8 - bit_off;
            let take = if bits_left < avail { bits_left } else { avail };
            let shift = avail - take;
            let chunk = ((value >> (bits_left - take)) & ((1u64 << take) - 1)) as u8;
            // Clear the destination bits, then OR in the new ones. This lets
            // callers pass a freshly-allocated zeroed buffer or a reused one
            // and get well-defined output either way.
            let mask = (((1u32 << take) - 1) as u8) << shift;
            self.bytes[byte_idx] = (self.bytes[byte_idx] & !mask) | (chunk << shift);
            self.bit_pos += take as usize;
            bits_left -= take;
        }
        Ok(())
    }

    /// Write one bit: `1` for `true`, `0` for `false`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::BufferFull`] if no bits remain in the buffer.
    ///
    /// # Example
    ///
    /// ```
    /// use wire_codec::BitWriter;
    ///
    /// let mut storage = [0u8; 1];
    /// let mut w = BitWriter::new(&mut storage);
    /// w.write_bool(true).unwrap();
    /// w.write_bool(false).unwrap();
    /// w.write_bool(true).unwrap();
    /// assert_eq!(w.finish(), 1);
    /// assert_eq!(storage[0], 0b1010_0000);
    /// ```
    #[inline]
    pub fn write_bool(&mut self, value: bool) -> Result<()> {
        self.write_bits(u64::from(value), 1)
    }

    /// Write `value` as an `n`-bit two's-complement field.
    ///
    /// # Errors
    ///
    /// - [`Error::BitOverflow`] if `n` is `0`, greater than [`MAX_BIT_WIDTH`],
    ///   or `value` is outside `-(2^(n-1))..2^(n-1)`.
    /// - [`Error::BufferFull`] if fewer than `n` bits remain in the buffer.
    ///
    /// # Example
    ///
    /// ```
    /// use wire_codec::{BitReader, BitWriter, Error};
    ///
    /// let mut storage = [0u8; 1];
    /// let mut w = BitWriter::new(&mut storage);
    /// w.write_signed_bits(-3, 3).unwrap();
    /// assert_eq!(w.write_signed_bits(4, 3), Err(Error::BitOverflow));
    /// assert_eq!(w.finish(), 1);
    /// assert_eq!(BitReader::new(&storage).read_signed_bits(3).unwrap(), -3);
    /// ```
    #[inline]
    pub fn write_signed_bits(&mut self, value: i64, n: u32) -> Result<()> {
        let raw = signed_to_bits(value, n).ok_or(Error::BitOverflow)?;
        self.write_bits(raw, n)
    }

    /// Finalize writes and return the number of whole bytes consumed.
    ///
    /// A partially-filled trailing byte counts as one full byte, and its
    /// remaining low-order bits are cleared to zero.
    #[inline]
    pub fn finish(mut self) -> usize {
        self.zero_trailing_bits();
        self.bit_pos.div_ceil(8)
    }
}

/// Number of addressable bits in a slice of `len` bytes.
///
/// Saturates so that `len * 8` cannot overflow on 32-bit targets: slices
/// larger than `usize::MAX / 8` bytes expose only their first
/// `usize::MAX / 8` bytes to the bit cursors. The result is always a
/// multiple of 8 and at most `usize::MAX - 7`.
#[inline]
const fn bit_capacity(len: usize) -> usize {
    let max_bytes = usize::MAX / 8;
    let bytes = if len < max_bytes { len } else { max_bytes };
    bytes * 8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pack_unpack_round_trip() {
        let mut storage = [0u8; 4];
        let mut w = BitWriter::new(&mut storage);
        w.write_bits(0b1011, 4).unwrap();
        w.write_bits(0b1100_1010, 8).unwrap();
        w.write_bits(0b1111_0000, 8).unwrap();
        let n = w.finish();
        assert_eq!(n, 3);

        let mut r = BitReader::new(&storage[..n]);
        assert_eq!(r.read_bits(4).unwrap(), 0b1011);
        assert_eq!(r.read_bits(8).unwrap(), 0b1100_1010);
        assert_eq!(r.read_bits(8).unwrap(), 0b1111_0000);
    }

    #[test]
    fn write_rejects_value_overflow() {
        let mut storage = [0u8; 1];
        let mut w = BitWriter::new(&mut storage);
        // 0b1_0000 needs 5 bits but only 4 were requested.
        assert_eq!(w.write_bits(0b1_0000, 4), Err(Error::BitOverflow));
    }

    #[test]
    fn padding_bits_are_zeroed_on_reused_buffer() {
        // Regression test: align and finish must clear stale bits.
        let mut storage = [0xFFu8; 2];
        let mut w = BitWriter::new(&mut storage);
        w.write_bits(0b1, 1).unwrap();
        w.align_to_byte().unwrap();
        w.write_bits(0b0, 1).unwrap();
        assert_eq!(w.finish(), 2);
        assert_eq!(storage, [0b1000_0000, 0b0000_0000]);
    }

    #[test]
    fn finish_leaves_bytes_past_the_prefix_untouched() {
        let mut storage = [0xAAu8; 3];
        let mut w = BitWriter::new(&mut storage);
        w.write_bits(0b11, 2).unwrap();
        assert_eq!(w.finish(), 1);
        assert_eq!(storage, [0b1100_0000, 0xAA, 0xAA]);
    }

    #[test]
    fn align_when_already_aligned_is_a_no_op() {
        let mut storage = [0xFFu8; 1];
        let mut w = BitWriter::new(&mut storage);
        w.align_to_byte().unwrap();
        assert_eq!(w.bits_written(), 0);
        w.write_bits(0xA5, 8).unwrap();
        w.align_to_byte().unwrap();
        assert_eq!(w.bits_written(), 8);
        assert_eq!(w.finish(), 1);
        assert_eq!(storage, [0xA5]);
    }

    #[test]
    fn reader_align_to_byte() {
        let mut r = BitReader::new(&[0b1010_0000, 0xCD]);
        r.align_to_byte();
        assert_eq!(r.bits_consumed(), 0, "aligned cursor does not move");
        assert_eq!(r.read_bits(3).unwrap(), 0b101);
        r.align_to_byte();
        assert_eq!(r.bits_consumed(), 8);
        assert_eq!(r.bits_remaining(), 8);
        assert_eq!(r.read_bits(8).unwrap(), 0xCD);
        r.align_to_byte();
        assert_eq!(r.bits_consumed(), 16, "align at end of input stays put");
        assert_eq!(r.read_bits(1), Err(Error::UnexpectedEof));
    }

    #[test]
    fn width_validation() {
        let mut r = BitReader::new(&[0xFF; 16]);
        assert_eq!(r.read_bits(0), Err(Error::BitOverflow));
        assert_eq!(r.read_bits(65), Err(Error::BitOverflow));
        assert_eq!(r.read_signed_bits(0), Err(Error::BitOverflow));
        assert_eq!(r.bits_consumed(), 0);

        let mut storage = [0u8; 16];
        let mut w = BitWriter::new(&mut storage);
        assert_eq!(w.write_bits(0, 0), Err(Error::BitOverflow));
        assert_eq!(w.write_bits(0, 65), Err(Error::BitOverflow));
        assert_eq!(w.write_signed_bits(0, 0), Err(Error::BitOverflow));
        assert_eq!(w.write_signed_bits(0, 65), Err(Error::BitOverflow));
        assert_eq!(w.bits_written(), 0);
    }

    #[test]
    fn full_width_fields_at_unaligned_offsets() {
        for offset in 0..8u32 {
            let mut storage = [0u8; 10];
            let mut w = BitWriter::new(&mut storage);
            if offset > 0 {
                w.write_bits(0, offset).unwrap();
            }
            w.write_bits(0xDEAD_BEEF_0123_4567, 64).unwrap();
            assert_eq!(w.write_bits(u64::MAX, 64), Err(Error::BufferFull));
            let n = w.finish();
            let mut r = BitReader::new(&storage[..n]);
            if offset > 0 {
                assert_eq!(r.read_bits(offset).unwrap(), 0);
            }
            assert_eq!(r.read_bits(64).unwrap(), 0xDEAD_BEEF_0123_4567);
        }
    }

    #[test]
    fn reads_and_writes_spanning_several_bytes() {
        let mut storage = [0u8; 4];
        let mut w = BitWriter::new(&mut storage);
        w.write_bits(0b1, 1).unwrap();
        w.write_bits(0x1_2345, 17).unwrap();
        w.write_bits(0x3FFF, 14).unwrap();
        assert_eq!(w.bits_written(), 32);
        assert_eq!(w.write_bool(true), Err(Error::BufferFull));
        assert_eq!(w.finish(), 4);
        let mut r = BitReader::new(&storage);
        assert_eq!(r.read_bits(1).unwrap(), 1);
        assert_eq!(r.read_bits(17).unwrap(), 0x1_2345);
        assert_eq!(r.read_bits(14).unwrap(), 0x3FFF);
        assert_eq!(r.read_bool(), Err(Error::UnexpectedEof));
    }

    #[test]
    fn signed_fields() {
        for n in [1u32, 2, 7, 8, 13, 63, 64] {
            let min = if n == 64 {
                i64::MIN
            } else {
                -(1i64 << (n - 1))
            };
            let max = if n == 64 {
                i64::MAX
            } else {
                (1i64 << (n - 1)) - 1
            };
            for v in [min, max, 0, -1] {
                let mut storage = [0u8; 8];
                let mut w = BitWriter::new(&mut storage);
                w.write_signed_bits(v, n).unwrap();
                let bytes = w.finish();
                let mut r = BitReader::new(&storage[..bytes]);
                assert_eq!(r.read_signed_bits(n).unwrap(), v, "n={n} v={v}");
            }
            if n < 64 {
                let mut storage = [0u8; 8];
                let mut w = BitWriter::new(&mut storage);
                assert_eq!(w.write_signed_bits(max + 1, n), Err(Error::BitOverflow));
                assert_eq!(w.write_signed_bits(min - 1, n), Err(Error::BitOverflow));
            }
        }
        assert_eq!(sign_extend(0b1, 1), -1);
        assert_eq!(signed_to_bits(-1, 1), Some(1));
        assert_eq!(signed_to_bits(1, 1), None);
    }

    #[test]
    fn bit_capacity_saturates() {
        assert_eq!(bit_capacity(0), 0);
        assert_eq!(bit_capacity(3), 24);
        assert_eq!(bit_capacity(usize::MAX), (usize::MAX / 8) * 8);
        assert_eq!(bit_capacity(usize::MAX) % 8, 0);
    }

    #[test]
    fn align_rounds_up_to_byte() {
        let mut storage = [0u8; 2];
        let mut w = BitWriter::new(&mut storage);
        w.write_bits(0b101, 3).unwrap();
        w.align_to_byte().unwrap();
        w.write_bits(0xFF, 8).unwrap();
        assert_eq!(w.finish(), 2);
        assert_eq!(storage, [0b1010_0000, 0xFF]);
    }
}
