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
        (self.bytes.len() * 8) - self.bit_pos
    }

    /// Advance the cursor to the next byte boundary.
    #[inline]
    pub fn align_to_byte(&mut self) {
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
}

/// Writes packed bits to a caller-supplied byte slice, most-significant-bit first.
///
/// The cursor zero-fills bytes lazily as it advances, so callers can pass an
/// uninitialized-looking buffer and read out only the prefix returned by
/// [`BitWriter::finish`].
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
    /// # Errors
    ///
    /// Returns [`Error::BufferFull`] if rounding up would exceed the backing
    /// slice.
    pub fn align_to_byte(&mut self) -> Result<()> {
        let aligned = (self.bit_pos + 7) & !7;
        if aligned > self.bytes.len() * 8 {
            return Err(Error::BufferFull);
        }
        self.bit_pos = aligned;
        Ok(())
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
        if (n as usize) > self.bytes.len() * 8 - self.bit_pos {
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

    /// Finalize writes and return the number of whole bytes consumed.
    ///
    /// A partially-filled trailing byte counts as one full byte, with the
    /// remaining low-order bits left zero-padded.
    #[inline]
    pub fn finish(self) -> usize {
        self.bit_pos.div_ceil(8)
    }
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
