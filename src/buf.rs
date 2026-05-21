//! Zero-copy cursor primitives over borrowed byte slices.
//!
//! [`ReadBuf`] wraps an immutable `&[u8]` and tracks a read position. It hands
//! back sub-slices that borrow from the original buffer, so decoders never copy.
//! [`WriteBuf`] wraps a mutable `&mut [u8]` and tracks a write position into a
//! caller-owned region.
//!
//! Both cursors are `no_std`-compatible and allocation-free.

use crate::error::{Error, Result};

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
    pub const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, pos: 0 }
    }

    /// Current read offset within the backing slice.
    #[inline]
    pub const fn position(&self) -> usize {
        self.pos
    }

    /// Number of bytes still available for reading.
    #[inline]
    pub const fn remaining(&self) -> usize {
        self.bytes.len() - self.pos
    }

    /// Whether the cursor has consumed every byte of the backing slice.
    #[inline]
    pub const fn is_empty(&self) -> bool {
        self.pos >= self.bytes.len()
    }

    /// Slice of bytes that have not yet been consumed.
    #[inline]
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
        if end > self.bytes.len() {
            return Err(Error::UnexpectedEof);
        }
        Ok(&self.bytes[self.pos..end])
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

    /// Advance the cursor by `n` bytes without returning the skipped data.
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnexpectedEof`] when fewer than `n` bytes remain.
    #[inline]
    pub fn advance(&mut self, n: usize) -> Result<()> {
        let end = self.pos.checked_add(n).ok_or(Error::UnexpectedEof)?;
        if end > self.bytes.len() {
            return Err(Error::UnexpectedEof);
        }
        self.pos = end;
        Ok(())
    }

    /// Read a single byte.
    ///
    /// # Errors
    ///
    /// Returns [`Error::UnexpectedEof`] when the cursor is exhausted.
    #[inline]
    pub fn read_u8(&mut self) -> Result<u8> {
        if self.pos >= self.bytes.len() {
            return Err(Error::UnexpectedEof);
        }
        let value = self.bytes[self.pos];
        self.pos += 1;
        Ok(value)
    }

    /// Read a big-endian `u16`.
    #[inline]
    pub fn read_u16_be(&mut self) -> Result<u16> {
        let bytes = self.read_bytes(2)?;
        Ok(u16::from_be_bytes([bytes[0], bytes[1]]))
    }

    /// Read a little-endian `u16`.
    #[inline]
    pub fn read_u16_le(&mut self) -> Result<u16> {
        let bytes = self.read_bytes(2)?;
        Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
    }

    /// Read a big-endian `u32`.
    #[inline]
    pub fn read_u32_be(&mut self) -> Result<u32> {
        let bytes = self.read_bytes(4)?;
        Ok(u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    /// Read a little-endian `u32`.
    #[inline]
    pub fn read_u32_le(&mut self) -> Result<u32> {
        let bytes = self.read_bytes(4)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    /// Read a big-endian `u64`.
    #[inline]
    pub fn read_u64_be(&mut self) -> Result<u64> {
        let bytes = self.read_bytes(8)?;
        let mut arr = [0u8; 8];
        arr.copy_from_slice(bytes);
        Ok(u64::from_be_bytes(arr))
    }

    /// Read a little-endian `u64`.
    #[inline]
    pub fn read_u64_le(&mut self) -> Result<u64> {
        let bytes = self.read_bytes(8)?;
        let mut arr = [0u8; 8];
        arr.copy_from_slice(bytes);
        Ok(u64::from_le_bytes(arr))
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
#[derive(Debug)]
pub struct WriteBuf<'a> {
    bytes: &'a mut [u8],
    pos: usize,
}

impl<'a> WriteBuf<'a> {
    /// Wrap `bytes` in a new cursor positioned at offset zero.
    #[inline]
    pub fn new(bytes: &'a mut [u8]) -> Self {
        Self { bytes, pos: 0 }
    }

    /// Total capacity of the backing slice.
    #[inline]
    pub fn capacity(&self) -> usize {
        self.bytes.len()
    }

    /// Current write offset within the backing slice.
    #[inline]
    pub fn position(&self) -> usize {
        self.pos
    }

    /// Number of bytes still available for writing.
    #[inline]
    pub fn remaining(&self) -> usize {
        self.bytes.len() - self.pos
    }

    /// Borrow the prefix that has been written so far.
    #[inline]
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
        if end > self.bytes.len() {
            return Err(Error::BufferFull);
        }
        self.bytes[self.pos..end].copy_from_slice(src);
        self.pos = end;
        Ok(())
    }

    /// Write a single byte.
    #[inline]
    pub fn write_u8(&mut self, value: u8) -> Result<()> {
        if self.pos >= self.bytes.len() {
            return Err(Error::BufferFull);
        }
        self.bytes[self.pos] = value;
        self.pos += 1;
        Ok(())
    }

    /// Write a big-endian `u16`.
    #[inline]
    pub fn write_u16_be(&mut self, value: u16) -> Result<()> {
        self.write_bytes(&value.to_be_bytes())
    }

    /// Write a little-endian `u16`.
    #[inline]
    pub fn write_u16_le(&mut self, value: u16) -> Result<()> {
        self.write_bytes(&value.to_le_bytes())
    }

    /// Write a big-endian `u32`.
    #[inline]
    pub fn write_u32_be(&mut self, value: u32) -> Result<()> {
        self.write_bytes(&value.to_be_bytes())
    }

    /// Write a little-endian `u32`.
    #[inline]
    pub fn write_u32_le(&mut self, value: u32) -> Result<()> {
        self.write_bytes(&value.to_le_bytes())
    }

    /// Write a big-endian `u64`.
    #[inline]
    pub fn write_u64_be(&mut self, value: u64) -> Result<()> {
        self.write_bytes(&value.to_be_bytes())
    }

    /// Write a little-endian `u64`.
    #[inline]
    pub fn write_u64_le(&mut self, value: u64) -> Result<()> {
        self.write_bytes(&value.to_le_bytes())
    }

    /// Consume the cursor and return the prefix that has been written.
    #[inline]
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
    }

    #[test]
    fn read_buf_peek_does_not_advance() {
        let buf = ReadBuf::new(&[0xAA, 0xBB]);
        assert_eq!(buf.peek(2).unwrap(), &[0xAA, 0xBB]);
        assert_eq!(buf.position(), 0);
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
    fn write_buf_full() {
        let mut storage = [0u8; 2];
        let mut buf = WriteBuf::new(&mut storage);
        buf.write_u16_be(0x1234).unwrap();
        assert_eq!(buf.write_u8(0), Err(Error::BufferFull));
    }
}
