//! Length-prefixed framing.
//!
//! Each frame is preceded by a fixed-width unsigned integer that gives the
//! length of the payload in bytes. The width and endianness are chosen at
//! construction time.

use crate::buf::WriteBuf;
use crate::error::{Error, Result};
use crate::framing::{Frame, Framer};

/// Width of the length prefix, in bytes.
///
/// Marked `#[non_exhaustive]` so wider prefix widths can be added in future
/// minor releases without a major version bump.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LengthWidth {
    /// One-byte length prefix; payload up to 255 bytes.
    U8,
    /// Two-byte length prefix; payload up to 65 535 bytes.
    U16,
    /// Four-byte length prefix; payload up to 4 294 967 295 bytes.
    U32,
}

impl LengthWidth {
    /// Width of the prefix in bytes.
    #[inline]
    pub const fn header_size(self) -> usize {
        match self {
            LengthWidth::U8 => 1,
            LengthWidth::U16 => 2,
            LengthWidth::U32 => 4,
        }
    }

    /// Largest payload size encodable by this width.
    #[inline]
    pub const fn max_payload(self) -> u64 {
        match self {
            LengthWidth::U8 => u8::MAX as u64,
            LengthWidth::U16 => u16::MAX as u64,
            LengthWidth::U32 => u32::MAX as u64,
        }
    }
}

/// Byte order for the length prefix.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Endian {
    /// Most significant byte first (network order).
    Big,
    /// Least significant byte first.
    Little,
}

/// Length-prefixed framer.
///
/// # Example
///
/// ```
/// use wire_codec::WriteBuf;
/// use wire_codec::framing::{Endian, Framer, LengthPrefixed, LengthWidth};
///
/// let framer = LengthPrefixed::new(LengthWidth::U16, Endian::Big);
///
/// let mut out = [0u8; 16];
/// let mut buf = WriteBuf::new(&mut out);
/// framer.write_frame(b"hello", &mut buf).unwrap();
/// let written = buf.position();
/// assert_eq!(&out[..written], &[0x00, 0x05, b'h', b'e', b'l', b'l', b'o']);
///
/// let frame = framer.next_frame(&out[..written]).unwrap().unwrap();
/// assert_eq!(frame.payload(), b"hello");
/// assert_eq!(frame.consumed(), 7);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LengthPrefixed {
    width: LengthWidth,
    endian: Endian,
    max_payload: u64,
}

impl LengthPrefixed {
    /// Build a framer using the given prefix width and endianness.
    ///
    /// The maximum payload size defaults to the largest value the prefix can
    /// encode. Use [`LengthPrefixed::with_max_payload`] to tighten it.
    #[inline]
    pub const fn new(width: LengthWidth, endian: Endian) -> Self {
        Self {
            width,
            endian,
            max_payload: width.max_payload(),
        }
    }

    /// Override the payload size limit enforced when reading and writing frames.
    ///
    /// The limit is clamped to [`LengthWidth::max_payload`] of the configured
    /// width.
    #[inline]
    #[must_use]
    pub const fn with_max_payload(mut self, max: u64) -> Self {
        let cap = self.width.max_payload();
        self.max_payload = if max < cap { max } else { cap };
        self
    }

    /// Configured prefix width.
    #[inline]
    pub const fn width(&self) -> LengthWidth {
        self.width
    }

    /// Configured endianness.
    #[inline]
    pub const fn endian(&self) -> Endian {
        self.endian
    }

    /// Configured maximum payload size in bytes.
    #[inline]
    pub const fn max_payload(&self) -> u64 {
        self.max_payload
    }

    fn read_prefix(&self, header: &[u8]) -> u64 {
        match (self.width, self.endian) {
            (LengthWidth::U8, _) => u64::from(header[0]),
            (LengthWidth::U16, Endian::Big) => {
                u64::from(u16::from_be_bytes([header[0], header[1]]))
            }
            (LengthWidth::U16, Endian::Little) => {
                u64::from(u16::from_le_bytes([header[0], header[1]]))
            }
            (LengthWidth::U32, Endian::Big) => u64::from(u32::from_be_bytes([
                header[0], header[1], header[2], header[3],
            ])),
            (LengthWidth::U32, Endian::Little) => u64::from(u32::from_le_bytes([
                header[0], header[1], header[2], header[3],
            ])),
        }
    }

    fn write_prefix(&self, len: u64, out: &mut WriteBuf<'_>) -> Result<()> {
        match (self.width, self.endian) {
            (LengthWidth::U8, _) => out.write_u8(len as u8),
            (LengthWidth::U16, Endian::Big) => out.write_u16_be(len as u16),
            (LengthWidth::U16, Endian::Little) => out.write_u16_le(len as u16),
            (LengthWidth::U32, Endian::Big) => out.write_u32_be(len as u32),
            (LengthWidth::U32, Endian::Little) => out.write_u32_le(len as u32),
        }
    }
}

impl Framer for LengthPrefixed {
    fn next_frame<'a>(&self, input: &'a [u8]) -> Result<Option<Frame<'a>>> {
        let header_size = self.width.header_size();
        if input.len() < header_size {
            return Ok(None);
        }
        let len = self.read_prefix(&input[..header_size]);
        if len > self.max_payload {
            return Err(Error::FrameTooLarge {
                len: len as usize,
                limit: self.max_payload as usize,
            });
        }
        let consumed = header_size + (len as usize);
        if input.len() < consumed {
            return Ok(None);
        }
        let payload = &input[header_size..consumed];
        Ok(Some(Frame::new(payload, consumed)))
    }

    fn write_frame(&self, payload: &[u8], out: &mut WriteBuf<'_>) -> Result<()> {
        let len = payload.len() as u64;
        if len > self.max_payload {
            return Err(Error::FrameTooLarge {
                len: payload.len(),
                limit: self.max_payload as usize,
            });
        }
        self.write_prefix(len, out)?;
        out.write_bytes(payload)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn u8_big_round_trip() {
        let framer = LengthPrefixed::new(LengthWidth::U8, Endian::Big);
        let mut out = [0u8; 32];
        let mut buf = WriteBuf::new(&mut out);
        framer.write_frame(b"abc", &mut buf).unwrap();
        let n = buf.position();
        assert_eq!(&out[..n], &[0x03, b'a', b'b', b'c']);

        let frame = framer.next_frame(&out[..n]).unwrap().unwrap();
        assert_eq!(frame.payload(), b"abc");
        assert_eq!(frame.consumed(), 4);
    }

    #[test]
    fn partial_frame_returns_none() {
        let framer = LengthPrefixed::new(LengthWidth::U16, Endian::Big);
        // Header advertises 10 bytes but only 3 follow.
        let input = &[0x00, 0x0A, 0x01, 0x02, 0x03];
        assert_eq!(framer.next_frame(input).unwrap(), None);
    }

    #[test]
    fn empty_input_returns_none() {
        let framer = LengthPrefixed::new(LengthWidth::U16, Endian::Big);
        assert_eq!(framer.next_frame(&[]).unwrap(), None);
    }

    #[test]
    fn exceeds_max_payload() {
        let framer = LengthPrefixed::new(LengthWidth::U16, Endian::Big).with_max_payload(4);
        let input = &[0x00, 0x05, 0, 0, 0, 0, 0];
        assert!(matches!(
            framer.next_frame(input),
            Err(Error::FrameTooLarge { len: 5, limit: 4 })
        ));
    }

    #[test]
    fn write_rejects_oversize_payload() {
        let framer = LengthPrefixed::new(LengthWidth::U8, Endian::Big).with_max_payload(3);
        let mut out = [0u8; 16];
        let mut buf = WriteBuf::new(&mut out);
        assert!(matches!(
            framer.write_frame(&[0u8; 4], &mut buf),
            Err(Error::FrameTooLarge { len: 4, limit: 3 })
        ));
    }
}
