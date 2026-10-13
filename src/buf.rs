//! Zero-copy cursor primitives over borrowed byte slices.
//!
//! [`ReadBuf`] wraps an immutable `&[u8]` and tracks a read position. It hands
//! back sub-slices that borrow from the original buffer, so decoders never copy.
//! [`WriteBuf`] wraps a mutable `&mut [u8]` and tracks a write position into a
//! caller-owned region.
//!
//! Both cursors are `no_std`-compatible and allocation-free. Every fallible
//! method is atomic: on error the position is unchanged and, for
//! [`WriteBuf`], nothing is written.

use crate::error::{Error, Result};

/// Generate fixed-width `ReadBuf` readers from `from_*_bytes`.
macro_rules! read_fixed {
    ($($(#[$attr:meta])* $name:ident -> $ty:ty, $conv:ident, $desc:literal, $size:literal;)*) => {$(
        #[doc = concat!("Read a ", $desc, ".")]
        ///
        /// # Errors
        ///
        #[doc = concat!("Returns [`Error::UnexpectedEof`] when fewer than ", $size, " bytes remain.")]
        $(#[$attr])*
        #[inline]
        pub fn $name(&mut self) -> Result<$ty> {
            self.read_array().map(<$ty>::$conv)
        }
    )*};
}

/// Generate fixed-width `WriteBuf` writers from `to_*_bytes`.
macro_rules! write_fixed {
    ($($(#[$attr:meta])* $name:ident($ty:ty), $conv:ident, $desc:literal, $size:literal;)*) => {$(
        #[doc = concat!("Write a ", $desc, ".")]
        ///
        /// # Errors
        ///
        #[doc = concat!("Returns [`Error::BufferFull`] if fewer than ", $size, " bytes remain.")]
        $(#[$attr])*
        #[inline]
        pub fn $name(&mut self, value: $ty) -> Result<()> {
            self.write_bytes(&value.$conv())
        }
    )*};
}

/// Cursor over an immutable byte slice with a tracked read position.
///
/// # Example
///
/// ```
/// use wire_codec::ReadBuf;
///
/// let mut buf = ReadBuf::new(&[0xDE, 0xAD, 0xBE, 0xEF]);
/// assert_eq!(buf.read_u16_be().unwrap(), 0xDEAD);
/// assert_eq!(buf.read_u16_be().unwrap(), 0xBEEF);
/// assert_eq!(buf.remaining(), 0);
/// ```
#[derive(Debug, Clone)]
pub struct ReadBuf<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> ReadBuf<'a> {
    /// Wrap `bytes` in a new cursor positioned at the start of the slice.
    #[inline]
    #[must_use]
    pub const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }

    /// Current read offset within the backing slice.
    ///
    /// Because every read is atomic, after an error this is the offset of the
    /// value that failed to decode, which makes it useful in diagnostics.
    #[inline]
    #[must_use]
    pub const fn position(&self) -> usize {
        self.pos
    }

    /// Number of bytes still available for reading.
    #[inline]
    #[must_use]
    pub const fn remaining(&self) -> usize {
        self.bytes.len() - self.pos
    }

    /// Whether the cursor has consumed every byte of the backing slice.
    #[inline]
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.pos >= self.bytes.len()
    }

    /// Slice of bytes that have not yet been consumed.
    #[inline]
    #[must_use]
    pub fn remaining_slice(&self) -> &'a [u8] {
        &self.bytes[self.pos..]
    }

    /// Look at the next `n` bytes without advancing the cursor.
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnexpectedEof`] when fewer than `n` bytes remain.
    #[inline]
    pub fn peek(&self, n: usize) -> Result<&'a [u8]> {
        let end = self.pos.checked_add(n).ok_or(Error::UnexpectedEof)?;
        self.bytes.get(self.pos..end).ok_or(Error::UnexpectedEof)
    }

    /// Look at the next byte without advancing the cursor.
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnexpectedEof`] when the cursor is exhausted.
    ///
    /// # Example
    ///
    /// ```
    /// use wire_codec::ReadBuf;
    ///
    /// let buf = ReadBuf::new(&[0x7F]);
    /// assert_eq!(buf.peek_u8().unwrap(), 0x7F);
    /// assert_eq!(buf.position(), 0);
    /// ```
    #[inline]
    pub fn peek_u8(&self) -> Result<u8> {
        self.bytes
            .get(self.pos)
            .copied()
            .ok_or(Error::UnexpectedEof)
    }

    /// Read the next `n` bytes and advance the cursor by `n`.
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnexpectedEof`] when fewer than `n` bytes remain.
    #[inline]
    pub fn read_bytes(&mut self, n: usize) -> Result<&'a [u8]> {
        let slice = self.peek(n)?;
        self.pos += n;
        Ok(slice)
    }

    /// Read the next `N` bytes as a fixed-size array (copied).
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnexpectedEof`] when fewer than `N` bytes remain.
    ///
    /// # Example
    ///
    /// ```
    /// use wire_codec::ReadBuf;
    ///
    /// let mut buf = ReadBuf::new(b"GIF89a...");
    /// let magic: [u8; 6] = buf.read_array().unwrap();
    /// assert_eq!(&magic, b"GIF89a");
    /// ```
    #[inline]
    pub fn read_array<const N: usize>(&mut self) -> Result<[u8; N]> {
        self.read_array_ref().copied()
    }

    /// Borrow the next `N` bytes as a fixed-size array reference (zero-copy).
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnexpectedEof`] when fewer than `N` bytes remain.
    ///
    /// # Example
    ///
    /// ```
    /// use wire_codec::ReadBuf;
    ///
    /// let data = [1, 2, 3, 4, 5];
    /// let mut buf = ReadBuf::new(&data);
    /// let head: &[u8; 4] = buf.read_array_ref().unwrap();
    /// assert_eq!(head, &[1, 2, 3, 4]);
    /// ```
    #[inline]
    pub fn read_array_ref<const N: usize>(&mut self) -> Result<&'a [u8; N]> {
        let array = self
            .remaining_slice()
            .first_chunk::<N>()
            .ok_or(Error::UnexpectedEof)?;
        self.pos += N;
        Ok(array)
    }

    /// Split off the next `n` bytes as an independent cursor and advance past
    /// them.
    ///
    /// Useful for a length-delimited sub-record: decode it from the returned
    /// cursor, then check [`ReadBuf::is_empty`] to make sure it was consumed
    /// exactly.
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnexpectedEof`] when fewer than `n` bytes remain.
    ///
    /// # Example
    ///
    /// ```
    /// use wire_codec::ReadBuf;
    ///
    /// let mut buf = ReadBuf::new(&[0x02, 0xAA, 0xBB, 0xCC]);
    /// let len = usize::from(buf.read_u8().unwrap());
    /// let mut record = buf.split_to(len).unwrap();
    /// assert_eq!(record.read_u16_be().unwrap(), 0xAABB);
    /// assert!(record.is_empty());
    /// assert_eq!(buf.remaining_slice(), &[0xCC]);
    /// ```
    #[inline]
    pub fn split_to(&mut self, n: usize) -> Result<ReadBuf<'a>> {
        self.read_bytes(n).map(ReadBuf::new)
    }

    /// Advance the cursor by `n` bytes without returning the skipped data.
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnexpectedEof`] when fewer than `n` bytes remain.
    #[inline]
    pub fn advance(&mut self, n: usize) -> Result<()> {
        self.read_bytes(n).map(|_| ())
    }

    /// Advance past `n` bytes that the caller has already validated.
    ///
    /// Crate-internal fast path for decoders that parse from
    /// [`ReadBuf::remaining_slice`] and commit only on success.
    #[inline]
    pub(crate) fn consume(&mut self, n: usize) {
        debug_assert!(n <= self.remaining());
        self.pos += n;
    }

    /// Read a single byte.
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnexpectedEof`] when the cursor is exhausted.
    #[inline]
    pub fn read_u8(&mut self) -> Result<u8> {
        let value = self.peek_u8()?;
        self.pos += 1;
        Ok(value)
    }

    /// Read a single byte as an `i8` (two's complement).
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnexpectedEof`] when the cursor is exhausted.
    #[inline]
    pub fn read_i8(&mut self) -> Result<i8> {
        self.read_u8().map(|b| b as i8)
    }

    read_fixed! {
        read_u16_be -> u16, from_be_bytes, "big-endian `u16`", "2";
        read_u16_le -> u16, from_le_bytes, "little-endian `u16`", "2";
        read_u32_be -> u32, from_be_bytes, "big-endian `u32`", "4";
        read_u32_le -> u32, from_le_bytes, "little-endian `u32`", "4";
        read_u64_be -> u64, from_be_bytes, "big-endian `u64`", "8";
        read_u64_le -> u64, from_le_bytes, "little-endian `u64`", "8";
        read_u128_be -> u128, from_be_bytes, "big-endian `u128`", "16";
        read_u128_le -> u128, from_le_bytes, "little-endian `u128`", "16";
        read_i16_be -> i16, from_be_bytes, "big-endian `i16`", "2";
        read_i16_le -> i16, from_le_bytes, "little-endian `i16`", "2";
        read_i32_be -> i32, from_be_bytes, "big-endian `i32`", "4";
        read_i32_le -> i32, from_le_bytes, "little-endian `i32`", "4";
        read_i64_be -> i64, from_be_bytes, "big-endian `i64`", "8";
        read_i64_le -> i64, from_le_bytes, "little-endian `i64`", "8";
        read_i128_be -> i128, from_be_bytes, "big-endian `i128`", "16";
        read_i128_le -> i128, from_le_bytes, "little-endian `i128`", "16";
        read_f32_be -> f32, from_be_bytes, "big-endian IEEE 754 `f32`", "4";
        read_f32_le -> f32, from_le_bytes, "little-endian IEEE 754 `f32`", "4";
        read_f64_be -> f64, from_be_bytes, "big-endian IEEE 754 `f64`", "8";
        read_f64_le -> f64, from_le_bytes, "little-endian IEEE 754 `f64`", "8";
    }
}

/// Cursor over a mutable byte slice with a tracked write position.
///
/// # Example
///
/// ```
/// use wire_codec::WriteBuf;
///
/// let mut storage = [0u8; 4];
/// let mut buf = WriteBuf::new(&mut storage);
/// buf.write_u16_be(0xDEAD).unwrap();
/// buf.write_u16_be(0xBEEF).unwrap();
/// assert_eq!(buf.position(), 4);
/// assert_eq!(&storage, &[0xDE, 0xAD, 0xBE, 0xEF]);
/// ```
///
/// While a `WriteBuf` is alive it mutably borrows the storage, so read the
/// encoded bytes back through [`WriteBuf::written`] (or capture
/// [`WriteBuf::position`] in a local first) rather than indexing the storage
/// directly:
///
/// ```
/// use wire_codec::WriteBuf;
///
/// let mut storage = [0u8; 8];
/// let mut buf = WriteBuf::new(&mut storage);
/// buf.write_u8(1).unwrap();
/// let bytes: &[u8] = buf.written();
/// assert_eq!(bytes, &[1]);
/// ```
#[derive(Debug)]
pub struct WriteBuf<'a> {
    bytes: &'a mut [u8],
    pos: usize,
}

impl<'a> WriteBuf<'a> {
    /// Wrap `bytes` in a new cursor positioned at offset zero.
    #[inline]
    #[must_use]
    pub fn new(bytes: &'a mut [u8]) -> Self {
        Self { bytes, pos: 0 }
    }

    /// Total capacity of the backing slice.
    #[inline]
    #[must_use]
    pub fn capacity(&self) -> usize {
        self.bytes.len()
    }

    /// Current write offset within the backing slice.
    #[inline]
    #[must_use]
    pub fn position(&self) -> usize {
        self.pos
    }

    /// Number of bytes still available for writing.
    #[inline]
    #[must_use]
    pub fn remaining(&self) -> usize {
        self.bytes.len() - self.pos
    }

    /// Borrow the prefix that has been written so far.
    #[inline]
    #[must_use]
    pub fn written(&self) -> &[u8] {
        &self.bytes[..self.pos]
    }

    /// Append `src` to the buffer and advance the cursor.
    ///
    /// # Errors
    ///
    /// Returns [`Error::BufferFull`] if fewer than `src.len()` bytes remain.
    #[inline]
    pub fn write_bytes(&mut self, src: &[u8]) -> Result<()> {
        let end = self.pos.checked_add(src.len()).ok_or(Error::BufferFull)?;
        let dst = self.bytes.get_mut(self.pos..end).ok_or(Error::BufferFull)?;
        dst.copy_from_slice(src);
        self.pos = end;
        Ok(())
    }

    /// Write a single byte.
    ///
    /// # Errors
    ///
    /// Returns [`Error::BufferFull`] if no capacity remains.
    #[inline]
    pub fn write_u8(&mut self, value: u8) -> Result<()> {
        let slot = self.bytes.get_mut(self.pos).ok_or(Error::BufferFull)?;
        *slot = value;
        self.pos += 1;
        Ok(())
    }

    /// Write a single `i8` (two's complement).
    ///
    /// # Errors
    ///
    /// Returns [`Error::BufferFull`] if no capacity remains.
    #[inline]
    pub fn write_i8(&mut self, value: i8) -> Result<()> {
        self.write_u8(value as u8)
    }

    write_fixed! {
        write_u16_be(u16), to_be_bytes, "big-endian `u16`", "2";
        write_u16_le(u16), to_le_bytes, "little-endian `u16`", "2";
        write_u32_be(u32), to_be_bytes, "big-endian `u32`", "4";
        write_u32_le(u32), to_le_bytes, "little-endian `u32`", "4";
        write_u64_be(u64), to_be_bytes, "big-endian `u64`", "8";
        write_u64_le(u64), to_le_bytes, "little-endian `u64`", "8";
        write_u128_be(u128), to_be_bytes, "big-endian `u128`", "16";
        write_u128_le(u128), to_le_bytes, "little-endian `u128`", "16";
        write_i16_be(i16), to_be_bytes, "big-endian `i16`", "2";
        write_i16_le(i16), to_le_bytes, "little-endian `i16`", "2";
        write_i32_be(i32), to_be_bytes, "big-endian `i32`", "4";
        write_i32_le(i32), to_le_bytes, "little-endian `i32`", "4";
        write_i64_be(i64), to_be_bytes, "big-endian `i64`", "8";
        write_i64_le(i64), to_le_bytes, "little-endian `i64`", "8";
        write_i128_be(i128), to_be_bytes, "big-endian `i128`", "16";
        write_i128_le(i128), to_le_bytes, "little-endian `i128`", "16";
        write_f32_be(f32), to_be_bytes, "big-endian IEEE 754 `f32`", "4";
        write_f32_le(f32), to_le_bytes, "little-endian IEEE 754 `f32`", "4";
        write_f64_be(f64), to_be_bytes, "big-endian IEEE 754 `f64`", "8";
        write_f64_le(f64), to_le_bytes, "little-endian IEEE 754 `f64`", "8";
    }

    /// Mutable view of the unwritten tail of the buffer.
    ///
    /// Crate-internal: encoders write directly into this slice, then call
    /// [`WriteBuf::commit`] with the number of bytes they produced.
    #[inline]
    pub(crate) fn spare_mut(&mut self) -> &mut [u8] {
        &mut self.bytes[self.pos..]
    }

    /// Mark `n` bytes of [`WriteBuf::spare_mut`] as written.
    #[inline]
    pub(crate) fn commit(&mut self, n: usize) {
        debug_assert!(n <= self.remaining());
        self.pos += n;
    }

    /// Roll the write position back to `pos`, discarding everything written
    /// after it. Used to keep multi-step writes atomic on error.
    #[inline]
    pub(crate) fn rewind(&mut self, pos: usize) {
        debug_assert!(pos <= self.pos);
        self.pos = pos;
    }

    /// Consume the cursor and return the prefix that has been written.
    #[inline]
    #[must_use]
    pub fn into_written(self) -> &'a mut [u8] {
        &mut self.bytes[..self.pos]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_buf_round_trip() {
        let data = [0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08];
        let mut buf = ReadBuf::new(&data);
        assert_eq!(buf.read_u8().unwrap(), 0x01);
        assert_eq!(buf.read_u16_be().unwrap(), 0x0203);
        assert_eq!(buf.read_u32_be().unwrap(), 0x04050607);
        assert_eq!(buf.remaining(), 1);
        assert_eq!(buf.read_u8().unwrap(), 0x08);
        assert!(buf.is_empty());
    }

    #[test]
    fn read_buf_eof() {
        let mut buf = ReadBuf::new(&[0x01]);
        assert_eq!(buf.read_u8().unwrap(), 0x01);
        assert_eq!(buf.read_u8(), Err(Error::UnexpectedEof));
        assert_eq!(buf.read_i8(), Err(Error::UnexpectedEof));
        assert_eq!(buf.peek_u8(), Err(Error::UnexpectedEof));
    }

    #[test]
    fn read_buf_peek_does_not_advance() {
        let buf = ReadBuf::new(&[0xAA, 0xBB]);
        assert_eq!(buf.peek(2).unwrap(), &[0xAA, 0xBB]);
        assert_eq!(buf.position(), 0);
        assert_eq!(buf.peek(3), Err(Error::UnexpectedEof));
        assert_eq!(buf.peek(usize::MAX), Err(Error::UnexpectedEof));
    }

    #[test]
    fn every_fixed_width_read_in_both_orders() {
        let data: Vec<u8> = (1..=16).collect();
        let mut be = ReadBuf::new(&data);
        assert_eq!(
            be.read_u128_be().unwrap(),
            u128::from_be_bytes(data[..].try_into().unwrap())
        );
        let mut le = ReadBuf::new(&data);
        assert_eq!(
            le.read_u128_le().unwrap(),
            u128::from_le_bytes(data[..].try_into().unwrap())
        );

        let mut r = ReadBuf::new(&data);
        assert_eq!(r.read_u16_le().unwrap(), 0x0201);
        assert_eq!(r.read_u32_le().unwrap(), 0x0605_0403);
        assert_eq!(r.read_u64_le().unwrap(), 0x0E0D_0C0B_0A09_0807);
        assert_eq!(r.read_u16_be().unwrap(), 0x0F10);

        let mut r = ReadBuf::new(&data);
        assert_eq!(r.read_u64_be().unwrap(), 0x0102_0304_0506_0708);

        let neg = [0xFFu8; 16];
        let mut r = ReadBuf::new(&neg);
        assert_eq!(r.read_i8().unwrap(), -1);
        assert_eq!(r.read_i16_be().unwrap(), -1);
        assert_eq!(r.read_i16_le().unwrap(), -1);
        assert_eq!(r.read_i32_be().unwrap(), -1);
        assert_eq!(r.read_i32_le().unwrap(), -1);
        assert_eq!(r.remaining(), 3);
        let mut r = ReadBuf::new(&neg);
        assert_eq!(r.read_i64_be().unwrap(), -1);
        assert_eq!(r.read_i64_le().unwrap(), -1);
        let mut r = ReadBuf::new(&neg);
        assert_eq!(r.read_i128_be().unwrap(), -1);
        let mut r = ReadBuf::new(&neg);
        assert_eq!(r.read_i128_le().unwrap(), -1);
    }

    #[test]
    fn float_round_trip() {
        let mut storage = [0u8; 24];
        let mut w = WriteBuf::new(&mut storage);
        w.write_f32_be(1.5).unwrap();
        w.write_f32_le(-2.25).unwrap();
        w.write_f64_be(core::f64::consts::PI).unwrap();
        w.write_f64_le(f64::MIN_POSITIVE).unwrap();
        assert_eq!(&storage[..4], &1.5f32.to_be_bytes());
        let mut r = ReadBuf::new(&storage);
        assert_eq!(r.read_f32_be().unwrap(), 1.5);
        assert_eq!(r.read_f32_le().unwrap(), -2.25);
        assert_eq!(r.read_f64_be().unwrap(), core::f64::consts::PI);
        assert_eq!(r.read_f64_le().unwrap(), f64::MIN_POSITIVE);
        assert!(r.is_empty());
    }

    #[test]
    fn short_fixed_reads_are_atomic() {
        let mut r = ReadBuf::new(&[1, 2, 3]);
        assert_eq!(r.read_u32_be(), Err(Error::UnexpectedEof));
        assert_eq!(r.read_array::<4>(), Err(Error::UnexpectedEof));
        assert_eq!(r.read_array_ref::<4>(), Err(Error::UnexpectedEof));
        assert_eq!(
            r.split_to(4).map(|b| b.remaining()),
            Err(Error::UnexpectedEof)
        );
        assert_eq!(r.advance(4), Err(Error::UnexpectedEof));
        assert_eq!(r.advance(usize::MAX), Err(Error::UnexpectedEof));
        assert_eq!(r.position(), 0);
    }

    #[test]
    fn arrays_split_and_advance() {
        let data = [1u8, 2, 3, 4, 5, 6];
        let mut r = ReadBuf::new(&data);
        assert_eq!(r.read_array::<2>().unwrap(), [1, 2]);
        assert_eq!(r.read_array_ref::<1>().unwrap(), &[3]);
        r.advance(1).unwrap();
        assert_eq!(r.position(), 4);
        assert_eq!(r.remaining_slice(), &[5, 6]);
        let sub = r.split_to(2).unwrap();
        assert_eq!(sub.remaining_slice(), &[5, 6]);
        assert!(r.is_empty());
        assert_eq!(r.read_array::<0>().unwrap(), [0u8; 0]);
    }

    #[test]
    fn write_buf_round_trip() {
        let mut storage = [0u8; 8];
        let mut buf = WriteBuf::new(&mut storage);
        buf.write_u8(0x01).unwrap();
        buf.write_u16_be(0x0203).unwrap();
        buf.write_u32_le(0x07060504).unwrap();
        buf.write_u8(0x08).unwrap();
        assert_eq!(&storage, &[0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08]);
    }

    #[test]
    fn every_fixed_width_write() {
        let mut storage = [0u8; 64];
        let mut w = WriteBuf::new(&mut storage);
        w.write_i8(-2).unwrap();
        w.write_u16_le(0x0102).unwrap();
        w.write_i16_be(-2).unwrap();
        w.write_i16_le(-2).unwrap();
        w.write_u32_be(0x0102_0304).unwrap();
        w.write_i32_be(-2).unwrap();
        w.write_i32_le(-2).unwrap();
        w.write_u64_be(1).unwrap();
        w.write_u64_le(1).unwrap();
        w.write_i64_be(-2).unwrap();
        w.write_i64_le(-2).unwrap();
        assert_eq!(w.position(), 1 + 2 * 3 + 4 * 3 + 8 * 4);
        let n = w.position();
        let mut r = ReadBuf::new(&storage[..n]);
        assert_eq!(r.read_i8().unwrap(), -2);
        assert_eq!(r.read_u16_le().unwrap(), 0x0102);
        assert_eq!(r.read_i16_be().unwrap(), -2);
        assert_eq!(r.read_i16_le().unwrap(), -2);
        assert_eq!(r.read_u32_be().unwrap(), 0x0102_0304);
        assert_eq!(r.read_i32_be().unwrap(), -2);
        assert_eq!(r.read_i32_le().unwrap(), -2);
        assert_eq!(r.read_u64_be().unwrap(), 1);
        assert_eq!(r.read_u64_le().unwrap(), 1);
        assert_eq!(r.read_i64_be().unwrap(), -2);
        assert_eq!(r.read_i64_le().unwrap(), -2);

        let mut storage = [0u8; 64];
        let mut w = WriteBuf::new(&mut storage);
        w.write_u128_be(7).unwrap();
        w.write_u128_le(7).unwrap();
        w.write_i128_be(-7).unwrap();
        w.write_i128_le(-7).unwrap();
        let mut r = ReadBuf::new(&storage);
        assert_eq!(r.read_u128_be().unwrap(), 7);
        assert_eq!(r.read_u128_le().unwrap(), 7);
        assert_eq!(r.read_i128_be().unwrap(), -7);
        assert_eq!(r.read_i128_le().unwrap(), -7);
    }

    #[test]
    fn write_buf_full() {
        let mut storage = [0u8; 2];
        let mut buf = WriteBuf::new(&mut storage);
        buf.write_u16_be(0x1234).unwrap();
        assert_eq!(buf.write_u8(0), Err(Error::BufferFull));
        assert_eq!(buf.write_i8(0), Err(Error::BufferFull));
        assert_eq!(buf.write_bytes(&[]), Ok(()));
        assert_eq!(buf.write_bytes(&[0]), Err(Error::BufferFull));
    }

    #[test]
    fn write_buf_accessors() {
        let mut storage = [0u8; 4];
        let mut buf = WriteBuf::new(&mut storage);
        assert_eq!(buf.capacity(), 4);
        assert_eq!(buf.remaining(), 4);
        buf.write_bytes(&[9, 8, 7, 6]).unwrap();
        assert_eq!(buf.remaining(), 0);
        assert_eq!(buf.write_u32_be(0), Err(Error::BufferFull));
        assert_eq!(buf.position(), 4);
        let written = buf.into_written();
        written[0] = 1;
        assert_eq!(storage, [1, 8, 7, 6]);
    }
}
