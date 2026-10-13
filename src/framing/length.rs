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
    /// Three-byte length prefix; payload up to 16 777 215 bytes.
    ///
    /// Used by HTTP/2 frame headers, TLS handshake messages, and MySQL
    /// packets.
    U24,
    /// Eight-byte length prefix; payload up to `u64::MAX` bytes.
    ///
    /// On targets where `usize` is narrower than 64 bits, a length that does
    /// not fit in `usize` is reported as [`Error::FrameTooLarge`].
    U64,
}

impl LengthWidth {
    /// Width of the prefix in bytes.
    ///
    /// # Example
    ///
    /// ```
    /// use wire_codec::framing::LengthWidth;
    /// assert_eq!(LengthWidth::U24.header_size(), 3);
    /// ```
    #[inline]
    pub const fn header_size(self) -> usize {
        match self {
            LengthWidth::U8 => 1,
            LengthWidth::U16 => 2,
            LengthWidth::U24 => 3,
            LengthWidth::U32 => 4,
            LengthWidth::U64 => 8,
        }
    }

    /// Largest value the prefix can encode.
    ///
    /// # Example
    ///
    /// ```
    /// use wire_codec::framing::LengthWidth;
    /// assert_eq!(LengthWidth::U16.max_payload(), 65_535);
    /// ```
    #[inline]
    pub const fn max_payload(self) -> u64 {
        match self {
            LengthWidth::U8 => u8::MAX as u64,
            LengthWidth::U16 => u16::MAX as u64,
            LengthWidth::U24 => 0x00FF_FFFF,
            LengthWidth::U32 => u32::MAX as u64,
            LengthWidth::U64 => u64::MAX,
        }
    }
}

/// Byte order for the length prefix.
///
/// # Example
///
/// ```
/// use wire_codec::framing::{Endian, Framer, LengthPrefixed, LengthWidth};
///
/// let framer = LengthPrefixed::new(LengthWidth::U16, Endian::Little);
/// let frame = framer.next_frame(&[0x02, 0x00, b'o', b'k']).unwrap().unwrap();
/// assert_eq!(frame.payload(), b"ok");
/// ```
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
    ///
    /// # Example
    ///
    /// ```
    /// use wire_codec::framing::{Endian, LengthPrefixed, LengthWidth};
    ///
    /// let framer = LengthPrefixed::new(LengthWidth::U32, Endian::Big);
    /// assert_eq!(framer.max_payload(), u64::from(u32::MAX));
    /// ```
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
    ///
    /// # Example
    ///
    /// ```
    /// use wire_codec::framing::{Endian, LengthPrefixed, LengthWidth};
    ///
    /// let framer = LengthPrefixed::new(LengthWidth::U8, Endian::Big).with_max_payload(1_000);
    /// assert_eq!(framer.max_payload(), 255);
    /// ```
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

    /// Decode the prefix from exactly `header_size()` bytes.
    fn read_prefix(&self, header: &[u8]) -> u64 {
        let mut be = [0u8; 8];
        // Right-align the prefix bytes in big-endian order, so every width
        // shares one conversion.
        let n = header.len();
        match self.endian {
            Endian::Big => be[8 - n..].copy_from_slice(header),
            Endian::Little => {
                for (dst, src) in be[8 - n..].iter_mut().rev().zip(header) {
                    *dst = *src;
                }
            }
        }
        u64::from_be_bytes(be)
    }

    /// Encode `value` (already checked to fit the width) as the prefix.
    fn prefix_bytes(&self, value: u64) -> ([u8; 8], usize) {
        let n = self.width.header_size();
        let be = value.to_be_bytes();
        let mut out = [0u8; 8];
        match self.endian {
            Endian::Big => out[..n].copy_from_slice(&be[8 - n..]),
            Endian::Little => {
                for (dst, src) in out[..n].iter_mut().zip(be[8 - n..].iter().rev()) {
                    *dst = *src;
                }
            }
        }
        (out, n)
    }

    /// Build a `FrameTooLarge` error, saturating both fields at `usize::MAX`
    /// so the conversion is lossless-or-saturating on 32-bit targets.
    fn too_large(&self, len: u64) -> Error {
        Error::FrameTooLarge {
            len: saturate_usize(len),
            limit: saturate_usize(self.max_payload),
        }
    }
}

impl Framer for LengthPrefixed {
    /// Extract one frame if the prefix and the full payload are present.
    ///
    /// # Errors
    ///
    /// Returns [`Error::FrameTooLarge`] if the prefix exceeds the configured
    /// limit, or the payload cannot be addressed on this target.
    fn next_frame<'a>(&self, input: &'a [u8]) -> Result<Option<Frame<'a>>> {
        let header_size = self.width.header_size();
        let Some(header) = input.get(..header_size) else {
            return Ok(None);
        };
        let len = self.read_prefix(header);
        if len > self.max_payload {
            return Err(self.too_large(len));
        }
        // On 32-bit targets a `u32` (or wider) length does not necessarily
        // fit in `usize`, and adding the header size can overflow, so both
        // steps are checked. A length that cannot be addressed can never be
        // buffered, so it is reported as too large rather than "need more".
        let consumed = usize::try_from(len)
            .ok()
            .and_then(|len| len.checked_add(header_size))
            .ok_or_else(|| self.too_large(len))?;
        let Some(frame) = input.get(header_size..consumed) else {
            return Ok(None);
        };
        Ok(Some(Frame::new(frame, consumed)))
    }

    /// Write the prefix and the payload.
    ///
    /// # Errors
    ///
    /// - [`Error::FrameTooLarge`] if the payload exceeds the configured
    ///   limit.
    /// - [`Error::BufferFull`] if `out` cannot hold the whole frame.
    fn write_frame(&self, payload: &[u8], out: &mut WriteBuf<'_>) -> Result<()> {
        let len = u64::try_from(payload.len()).unwrap_or(u64::MAX);
        if len > self.max_payload {
            return Err(self.too_large(len));
        }
        // `max_payload` never exceeds the width's maximum, so `len` fits.
        let (header, header_len) = self.prefix_bytes(len);
        out.write_bytes(&header[..header_len])?;
        out.write_bytes(payload)
    }
}

/// Convert `value` to `usize`, saturating at `usize::MAX` on targets where
/// `usize` is narrower than 64 bits.
#[inline]
fn saturate_usize(value: u64) -> usize {
    usize::try_from(value).unwrap_or(usize::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIDTHS: [LengthWidth; 5] = [
        LengthWidth::U8,
        LengthWidth::U16,
        LengthWidth::U24,
        LengthWidth::U32,
        LengthWidth::U64,
    ];

    #[test]
    fn huge_u32_prefix_never_panics() {
        // Regression test for the 32-bit overflow in `header_size + len`.
        // On 64-bit targets the frame is merely incomplete; on 32-bit targets
        // the length cannot be addressed and is reported as too large.
        let framer = LengthPrefixed::new(LengthWidth::U32, Endian::Big);
        for prefix in [0xFFFF_FFFFu32, 0xFFFF_FFFE, 0xFFFF_FFFD, 0xFFFF_FFFC] {
            let mut input = [0u8; 6];
            input[..4].copy_from_slice(&prefix.to_be_bytes());
            let result = framer.next_frame(&input);
            if usize::BITS > 32 {
                assert_eq!(result, Ok(None));
            } else {
                assert!(matches!(result, Err(Error::FrameTooLarge { .. })));
            }
        }
    }

    #[test]
    fn huge_u64_prefix_never_panics() {
        let framer = LengthPrefixed::new(LengthWidth::U64, Endian::Little);
        let input = [0xFF; 12];
        assert!(matches!(
            framer.next_frame(&input),
            Err(Error::FrameTooLarge { .. })
        ));
    }

    #[test]
    fn every_width_and_endian_round_trips() {
        for width in WIDTHS {
            for endian in [Endian::Big, Endian::Little] {
                let framer = LengthPrefixed::new(width, endian);
                let mut out = [0u8; 32];
                let mut buf = WriteBuf::new(&mut out);
                framer.write_frame(b"abc", &mut buf).unwrap();
                let n = buf.position();
                assert_eq!(n, width.header_size() + 3);
                let frame = framer.next_frame(&out[..n]).unwrap().unwrap();
                assert_eq!(frame.payload(), b"abc");
                assert_eq!(frame.consumed(), n);
            }
        }
    }

    #[test]
    fn prefix_byte_order() {
        let mut out = [0u8; 16];
        let cases: [(LengthWidth, Endian, &[u8]); 4] = [
            (LengthWidth::U24, Endian::Big, &[0x00, 0x01, 0x02]),
            (LengthWidth::U24, Endian::Little, &[0x02, 0x01, 0x00]),
            (
                LengthWidth::U64,
                Endian::Big,
                &[0, 0, 0, 0, 0, 0, 0x01, 0x02],
            ),
            (
                LengthWidth::U64,
                Endian::Little,
                &[0x02, 0x01, 0, 0, 0, 0, 0, 0],
            ),
        ];
        for (width, endian, header) in cases {
            let framer = LengthPrefixed::new(width, endian);
            assert_eq!(framer.read_prefix(header), 0x0102);
            let (bytes, n) = framer.prefix_bytes(0x0102);
            assert_eq!(&bytes[..n], header);
            let mut buf = WriteBuf::new(&mut out);
            assert_eq!(framer.write_frame(&[0u8; 2], &mut buf), Ok(()));
        }
    }

    #[test]
    fn u24_limits() {
        assert_eq!(LengthWidth::U24.max_payload(), (1 << 24) - 1);
        let framer = LengthPrefixed::new(LengthWidth::U24, Endian::Big);
        assert_eq!(framer.next_frame(&[0xFF, 0xFF, 0xFF, 0x00]), Ok(None));
    }

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
    fn header_only_input() {
        let framer = LengthPrefixed::new(LengthWidth::U16, Endian::Big);
        // Exactly the header, advertising an empty payload: a complete frame.
        let frame = framer.next_frame(&[0x00, 0x00]).unwrap().unwrap();
        assert_eq!(frame.payload(), b"");
        assert_eq!(frame.consumed(), 2);
        // Exactly the header, advertising one byte: incomplete.
        assert_eq!(framer.next_frame(&[0x00, 0x01]), Ok(None));
        // One byte short of the header.
        assert_eq!(framer.next_frame(&[0x00]), Ok(None));
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
        assert_eq!(
            framer.next_frame(input),
            Err(Error::FrameTooLarge { len: 5, limit: 4 })
        );
        // Exactly at the limit is accepted.
        let input = &[0x00, 0x04, 1, 2, 3, 4];
        assert_eq!(
            framer.next_frame(input).unwrap().unwrap().payload(),
            &[1, 2, 3, 4]
        );
    }

    #[test]
    fn write_limit_boundaries() {
        let framer = LengthPrefixed::new(LengthWidth::U8, Endian::Big).with_max_payload(3);
        let mut out = [0u8; 16];
        let mut buf = WriteBuf::new(&mut out);
        framer.write_frame(&[0u8; 3], &mut buf).unwrap();
        assert_eq!(
            framer.write_frame(&[0u8; 4], &mut buf),
            Err(Error::FrameTooLarge { len: 4, limit: 3 })
        );
        assert_eq!(buf.position(), 4);
    }

    #[test]
    fn saturate_usize_is_identity_when_it_fits() {
        assert_eq!(saturate_usize(42), 42);
        if usize::BITS == 32 {
            assert_eq!(saturate_usize(u64::MAX), usize::MAX);
        }
    }
}
