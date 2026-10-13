//! Checksum-trailer framing.
//!
//! [`Checksummed`] wraps any [`Framer`] and appends a fixed-size checksum
//! after each inner frame:
//!
//! ```text
//! +----------------------------+----------------+
//! | inner frame (header,       | checksum       |
//! | payload, delimiter, ...)   | (C::LEN bytes) |
//! +----------------------------+----------------+
//! ```
//!
//! The trailer sits outside the inner frame, so the inner framer is used
//! unchanged and nothing has to be copied or buffered on either path. For a
//! length-prefixed inner framer the prefix counts the payload only, which is
//! the layout of many serial and embedded protocols.

use crate::buf::WriteBuf;
use crate::checksum::Checksum;
use crate::error::{Error, Result};
use crate::framing::{Endian, Frame, Framer};

/// Which bytes the checksum covers.
///
/// # Example
///
/// ```
/// use wire_codec::checksum::Crc16CcittFalse;
/// use wire_codec::framing::{Checksummed, ChecksumScope, Endian, LengthPrefixed, LengthWidth};
///
/// let inner = LengthPrefixed::new(LengthWidth::U8, Endian::Big);
/// let framer = Checksummed::new(inner, Crc16CcittFalse, Endian::Big)
///     .with_scope(ChecksumScope::Payload);
/// assert_eq!(framer.scope(), ChecksumScope::Payload);
/// ```
#[non_exhaustive]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub enum ChecksumScope {
    /// Every byte of the inner frame: header, payload, and delimiter. A
    /// corrupted length prefix is caught as a checksum mismatch.
    #[default]
    Frame,
    /// The payload bytes only.
    Payload,
}

/// A framer decorator that appends and verifies a checksum trailer.
///
/// # Example
///
/// ```
/// use wire_codec::checksum::Crc32;
/// use wire_codec::framing::{Checksummed, Endian, Framer, LengthPrefixed, LengthWidth};
/// use wire_codec::{Error, WriteBuf};
///
/// let inner = LengthPrefixed::bounded(LengthWidth::U16, Endian::Big);
/// let framer = Checksummed::new(inner, Crc32, Endian::Little);
///
/// let mut out = [0u8; 32];
/// let mut buf = WriteBuf::new(&mut out);
/// framer.write_frame(b"ping", &mut buf).unwrap();
/// let n = buf.position();
/// assert_eq!(n, 2 + 4 + 4); // prefix + payload + CRC-32
///
/// let frame = framer.next_frame(&out[..n]).unwrap().unwrap();
/// assert_eq!(frame.payload(), b"ping");
/// assert_eq!(frame.consumed(), n);
///
/// out[3] ^= 0x01; // flip one payload bit in transit
/// assert_eq!(framer.next_frame(&out[..n]), Err(Error::ChecksumMismatch));
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Checksummed<F, C> {
    inner: F,
    checksum: C,
    endian: Endian,
    scope: ChecksumScope,
}

impl<F: Framer, C: Checksum> Checksummed<F, C> {
    /// Wrap `inner`, appending `checksum` in `endian` byte order. The
    /// checksum covers the whole inner frame ([`ChecksumScope::Frame`]).
    #[inline]
    #[must_use]
    pub const fn new(inner: F, checksum: C, endian: Endian) -> Self {
        const {
            assert!(C::LEN >= 1 && C::LEN <= 8, "Checksum::LEN must be in 1..=8");
        }
        Self {
            inner,
            checksum,
            endian,
            scope: ChecksumScope::Frame,
        }
    }

    /// Choose which bytes the checksum covers.
    #[inline]
    #[must_use]
    pub const fn with_scope(mut self, scope: ChecksumScope) -> Self {
        self.scope = scope;
        self
    }

    /// The wrapped framer.
    #[inline]
    #[must_use]
    pub const fn inner(&self) -> &F {
        &self.inner
    }

    /// The configured checksum.
    #[inline]
    #[must_use]
    pub const fn checksum(&self) -> &C {
        &self.checksum
    }

    /// Byte order of the trailer.
    #[inline]
    #[must_use]
    pub const fn endian(&self) -> Endian {
        self.endian
    }

    /// Which bytes the checksum covers.
    #[inline]
    #[must_use]
    pub const fn scope(&self) -> ChecksumScope {
        self.scope
    }

    fn trailer(&self, value: u64) -> [u8; 8] {
        let mut out = [0u8; 8];
        match self.endian {
            Endian::Big => out[..C::LEN].copy_from_slice(&value.to_be_bytes()[8 - C::LEN..]),
            Endian::Little => out[..C::LEN].copy_from_slice(&value.to_le_bytes()[..C::LEN]),
        }
        out
    }

    fn verify<'a>(&self, input: &'a [u8], frame: Frame<'a>) -> Result<Option<Frame<'a>>> {
        let start = frame.consumed();
        // Saturating: a misbehaving custom inner framer could report a
        // `consumed` past the input; `get` then simply finds no trailer.
        let end = start.saturating_add(C::LEN);
        let Some(received) = input.get(start..end) else {
            return Ok(None);
        };
        let covered = match self.scope {
            ChecksumScope::Frame => &input[..start],
            ChecksumScope::Payload => frame.payload(),
        };
        let expected = self.trailer(self.checksum.compute(covered));
        if expected[..C::LEN] != *received {
            return Err(Error::ChecksumMismatch);
        }
        Ok(Some(Frame::new(frame.payload(), end)))
    }
}

impl<F: Framer, C: Checksum> Framer for Checksummed<F, C> {
    /// Extract an inner frame and verify the trailer that follows it.
    ///
    /// # Errors
    ///
    /// - [`Error::ChecksumMismatch`] if the trailer does not match.
    /// - Any error from the inner framer.
    #[inline]
    fn next_frame<'a>(&self, input: &'a [u8]) -> Result<Option<Frame<'a>>> {
        match self.inner.next_frame(input)? {
            Some(frame) => self.verify(input, frame),
            None => Ok(None),
        }
    }

    /// Write the inner frame followed by the checksum trailer.
    ///
    /// # Errors
    ///
    /// - [`Error::BufferFull`] if `out` cannot hold the frame and trailer.
    ///   When the inner framer reports [`Framer::max_frame_len`] (every
    ///   built-in framer does, exactly), that bound plus the trailer is
    ///   checked before anything is written.
    /// - Any error from the inner framer.
    ///
    /// The cursor is unchanged on error. With a built-in inner framer
    /// nothing is written either. With a custom inner framer that reports no
    /// size bound, a trailer that does not fit can leave the inner frame's
    /// bytes behind the (restored) cursor.
    fn write_frame(&self, payload: &[u8], out: &mut WriteBuf<'_>) -> Result<()> {
        if let Some(inner_max) = self.inner.max_frame_len(payload.len()) {
            if inner_max.saturating_add(C::LEN) > out.remaining() {
                return Err(Error::BufferFull);
            }
        }
        let start = out.position();
        self.inner.write_frame(payload, out)?;
        let covered = match self.scope {
            ChecksumScope::Frame => &out.written()[start..],
            ChecksumScope::Payload => payload,
        };
        let trailer = self.trailer(self.checksum.compute(covered));
        if let Err(e) = out.write_bytes(&trailer[..C::LEN]) {
            out.rewind(start);
            return Err(e);
        }
        Ok(())
    }

    /// Resumable variant: forwards the progress hint to the inner framer.
    ///
    /// # Errors
    ///
    /// Same contract as [`Framer::next_frame`] for this type.
    #[inline]
    fn next_frame_from<'a>(
        &self,
        input: &'a [u8],
        scanned: &mut usize,
    ) -> Result<Option<Frame<'a>>> {
        match self.inner.next_frame_from(input, scanned)? {
            Some(frame) => self.verify(input, frame),
            None => Ok(None),
        }
    }

    #[inline]
    fn max_frame_len(&self, payload_len: usize) -> Option<usize> {
        self.inner.max_frame_len(payload_len)?.checked_add(C::LEN)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::checksum::{Crc16CcittFalse, Crc32, Crc32c, crc16_ccitt_false};
    use crate::framing::{Delimited, LengthPrefixed, LengthWidth, StreamDecoder, VarintPrefixed};

    #[test]
    fn trailer_byte_order_and_coverage() {
        let inner = LengthPrefixed::new(LengthWidth::U8, Endian::Big);
        let mut out = [0u8; 16];

        let framer = Checksummed::new(inner, Crc16CcittFalse, Endian::Big);
        let mut buf = WriteBuf::new(&mut out);
        framer.write_frame(b"ab", &mut buf).unwrap();
        let crc = crc16_ccitt_false(&[0x02, b'a', b'b']).to_be_bytes();
        assert_eq!(buf.written(), &[0x02, b'a', b'b', crc[0], crc[1]]);

        let framer = framer.with_scope(ChecksumScope::Payload);
        let mut buf = WriteBuf::new(&mut out);
        framer.write_frame(b"ab", &mut buf).unwrap();
        let crc = crc16_ccitt_false(b"ab").to_be_bytes();
        assert_eq!(buf.written(), &[0x02, b'a', b'b', crc[0], crc[1]]);
        let frame = framer.next_frame(&out[..5]).unwrap().unwrap();
        assert_eq!(frame.payload(), b"ab");

        let framer = Checksummed::new(inner, Crc16CcittFalse, Endian::Little);
        let mut buf = WriteBuf::new(&mut out);
        framer.write_frame(b"ab", &mut buf).unwrap();
        let crc = crc16_ccitt_false(&[0x02, b'a', b'b']).to_le_bytes();
        assert_eq!(&buf.written()[3..], &crc);
    }

    #[test]
    fn corrupted_header_is_detected_with_frame_scope() {
        let inner = LengthPrefixed::new(LengthWidth::U8, Endian::Big);
        let framer = Checksummed::new(inner, Crc32, Endian::Big);
        let mut out = [0u8; 16];
        let mut buf = WriteBuf::new(&mut out);
        framer.write_frame(b"abc", &mut buf).unwrap();
        let n = buf.position();
        out[0] = 2; // length 3 -> 2: the inner frame still parses
        assert_eq!(framer.next_frame(&out[..n]), Err(Error::ChecksumMismatch));
    }

    #[test]
    fn incomplete_trailer_needs_more_input() {
        let framer = Checksummed::new(VarintPrefixed::new(), Crc32c, Endian::Big);
        let mut out = [0u8; 16];
        let mut buf = WriteBuf::new(&mut out);
        framer.write_frame(b"xyz", &mut buf).unwrap();
        let n = buf.position();
        assert_eq!(Some(n), framer.max_frame_len(3));
        for cut in 0..n {
            assert_eq!(framer.next_frame(&out[..cut]), Ok(None), "cut={cut}");
        }
        assert_eq!(framer.next_frame(&out[..n]).unwrap().unwrap().consumed(), n);
    }

    #[test]
    fn write_is_atomic_when_trailer_does_not_fit() {
        let inner = LengthPrefixed::new(LengthWidth::U8, Endian::Big);
        let framer = Checksummed::new(inner, Crc32, Endian::Big);
        // Room for the inner frame (4 bytes) but not the 4-byte trailer.
        let mut out = [0xEEu8; 6];
        let mut buf = WriteBuf::new(&mut out);
        assert_eq!(framer.write_frame(b"abc", &mut buf), Err(Error::BufferFull));
        assert_eq!(buf.position(), 0);
        assert_eq!(out, [0xEE; 6], "nothing is written on error");
        let mut big = [0u8; 16];
        let mut buf = WriteBuf::new(&mut big);
        // Inner framer errors propagate unchanged.
        let framer = Checksummed::new(inner.with_max_payload(1), Crc32, Endian::Big);
        assert!(matches!(
            framer.write_frame(b"abc", &mut buf),
            Err(Error::FrameTooLarge { .. })
        ));
    }

    #[test]
    fn delimited_inner_streams_linearly() {
        let inner = Delimited::bounded(b"\n").unwrap();
        let framer = Checksummed::new(inner, Crc16CcittFalse, Endian::Big);
        let mut wire = [0u8; 64];
        let mut buf = WriteBuf::new(&mut wire);
        framer.write_frame(b"hello", &mut buf).unwrap();
        framer.write_frame(b"world", &mut buf).unwrap();
        let n = buf.position();

        let mut decoder = StreamDecoder::new(framer);
        let mut start = 0;
        let mut frames = Vec::new();
        for end in 1..=n {
            while let Some(frame) = decoder.decode(&wire[start..end]).unwrap() {
                frames.push(frame.payload().to_vec());
                start += frame.consumed();
            }
        }
        assert_eq!(frames, [b"hello".to_vec(), b"world".to_vec()]);
        assert_eq!(start, n);
        assert_eq!(decoder.framer().inner().delimiter(), b"\n");
        assert_eq!(decoder.framer().checksum(), &Crc16CcittFalse);
        assert_eq!(decoder.framer().endian(), Endian::Big);
        assert_eq!(decoder.framer().scope(), ChecksumScope::Frame);
    }

    #[test]
    fn default_scope_is_frame() {
        assert_eq!(ChecksumScope::default(), ChecksumScope::Frame);
    }
}
