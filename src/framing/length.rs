//! Length-prefixed framing.
//!
//! Each frame is preceded by a fixed-width unsigned integer that gives the
//! length of the payload in bytes. The width and endianness are chosen at
//! construction time. An optional length adjustment covers protocols whose
//! length field counts something other than exactly the payload (for example
//! "length includes the header").
//!
//! # Limits
//!
//! [`LengthPrefixed::new`] keeps the 1.0 behaviour: the payload limit is the
//! largest value the prefix can encode (4 GiB for [`LengthWidth::U32`]). A
//! caller that buffers until a frame is complete would buffer that much for a
//! hostile peer. For untrusted input, use [`LengthPrefixed::bounded`] (a
//! [`LengthPrefixed::DEFAULT_MAX_PAYLOAD`] cap) or
//! [`LengthPrefixed::with_max_payload`].

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
    #[must_use]
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
    #[must_use]
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
    adjustment: i64,
}

impl LengthPrefixed {
    /// Payload limit applied by [`LengthPrefixed::bounded`]: 8 MiB.
    ///
    /// Matches the default of `tokio_util::codec::LengthDelimitedCodec`.
    pub const DEFAULT_MAX_PAYLOAD: u64 = 8 * 1024 * 1024;

    /// Build a framer using the given prefix width and endianness.
    ///
    /// The maximum payload size defaults to the largest value the prefix can
    /// encode. Use [`LengthPrefixed::bounded`] or
    /// [`LengthPrefixed::with_max_payload`] for untrusted input.
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
    #[must_use]
    pub const fn new(width: LengthWidth, endian: Endian) -> Self {
        Self {
            width,
            endian,
            max_payload: width.max_payload(),
            adjustment: 0,
        }
    }

    /// Build a framer capped at [`LengthPrefixed::DEFAULT_MAX_PAYLOAD`]
    /// payload bytes (or the width's own maximum, if smaller).
    ///
    /// This is the recommended constructor for input from untrusted peers.
    ///
    /// # Example
    ///
    /// ```
    /// use wire_codec::framing::{Endian, LengthPrefixed, LengthWidth};
    ///
    /// let framer = LengthPrefixed::bounded(LengthWidth::U32, Endian::Big);
    /// assert_eq!(framer.max_payload(), LengthPrefixed::DEFAULT_MAX_PAYLOAD);
    ///
    /// // A u16 prefix cannot describe more than 65 535 bytes anyway.
    /// let small = LengthPrefixed::bounded(LengthWidth::U16, Endian::Big);
    /// assert_eq!(small.max_payload(), 65_535);
    /// ```
    #[inline]
    #[must_use]
    pub const fn bounded(width: LengthWidth, endian: Endian) -> Self {
        Self::new(width, endian).with_max_payload(Self::DEFAULT_MAX_PAYLOAD)
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

    /// Set the length adjustment: the payload length is the prefix value
    /// plus `adjustment`.
    ///
    /// The default is `0` (the prefix counts exactly the payload bytes). When
    /// the prefix counts its own bytes as well, use `-(header_size)`; when it
    /// counts a fixed trailer that the caller handles separately, use the
    /// trailer size. This mirrors `length_adjustment` in
    /// `tokio_util::codec::LengthDelimitedCodec`.
    ///
    /// On decode, a prefix that adjusts to a negative length yields
    /// [`Error::InvalidLengthPrefix`]. On encode, a payload whose adjusted
    /// prefix value would be negative also yields
    /// [`Error::InvalidLengthPrefix`], and one whose prefix value would not
    /// fit in the width yields [`Error::FrameTooLarge`].
    ///
    /// The payload limit ([`LengthPrefixed::max_payload`]) is counted in
    /// payload bytes and is capped at the width's largest prefix value,
    /// independently of the adjustment. Two consequences:
    ///
    /// - A negative adjustment lowers the largest payload that can be
    ///   written to `width maximum + adjustment` (254 bytes for a
    ///   self-counting `U8` prefix). `write_frame` reports that effective
    ///   bound as the `limit` of [`Error::FrameTooLarge`].
    /// - A positive adjustment does not raise the limit: payloads longer
    ///   than [`LengthWidth::max_payload`] are rejected even though the
    ///   adjusted prefix could describe them.
    ///
    /// # Example
    ///
    /// ```
    /// use wire_codec::WriteBuf;
    /// use wire_codec::framing::{Endian, Framer, LengthPrefixed, LengthWidth};
    ///
    /// // The 2-byte length field counts itself.
    /// let framer = LengthPrefixed::new(LengthWidth::U16, Endian::Big).with_length_adjustment(-2);
    ///
    /// let mut out = [0u8; 8];
    /// let mut buf = WriteBuf::new(&mut out);
    /// framer.write_frame(b"abc", &mut buf).unwrap();
    /// assert_eq!(buf.written(), &[0x00, 0x05, b'a', b'b', b'c']);
    ///
    /// let frame = framer.next_frame(&out[..5]).unwrap().unwrap();
    /// assert_eq!(frame.payload(), b"abc");
    /// ```
    #[inline]
    #[must_use]
    pub const fn with_length_adjustment(mut self, adjustment: i64) -> Self {
        self.adjustment = adjustment;
        self
    }

    /// Configured prefix width.
    #[inline]
    #[must_use]
    pub const fn width(&self) -> LengthWidth {
        self.width
    }

    /// Configured endianness.
    #[inline]
    #[must_use]
    pub const fn endian(&self) -> Endian {
        self.endian
    }

    /// Configured maximum payload size in bytes.
    #[inline]
    #[must_use]
    pub const fn max_payload(&self) -> u64 {
        self.max_payload
    }

    /// Configured length adjustment (see
    /// [`LengthPrefixed::with_length_adjustment`]).
    #[inline]
    #[must_use]
    pub const fn length_adjustment(&self) -> i64 {
        self.adjustment
    }

    /// Decode the prefix from the first `header_size()` bytes of `input`,
    /// or `None` if fewer bytes are available.
    #[inline]
    fn read_prefix(&self, input: &[u8]) -> Option<u64> {
        let big = matches!(self.endian, Endian::Big);
        Some(match self.width {
            LengthWidth::U8 => u64::from(*input.first()?),
            LengthWidth::U16 => {
                let b: [u8; 2] = *input.first_chunk()?;
                u64::from(if big {
                    u16::from_be_bytes(b)
                } else {
                    u16::from_le_bytes(b)
                })
            }
            LengthWidth::U24 => {
                let [a, b, c] = *input.first_chunk::<3>()?;
                let word = if big { [0, a, b, c] } else { [a, b, c, 0] };
                u64::from(if big {
                    u32::from_be_bytes(word)
                } else {
                    u32::from_le_bytes(word)
                })
            }
            LengthWidth::U32 => {
                let b: [u8; 4] = *input.first_chunk()?;
                u64::from(if big {
                    u32::from_be_bytes(b)
                } else {
                    u32::from_le_bytes(b)
                })
            }
            LengthWidth::U64 => {
                let b: [u8; 8] = *input.first_chunk()?;
                if big {
                    u64::from_be_bytes(b)
                } else {
                    u64::from_le_bytes(b)
                }
            }
        })
    }

    /// Encode `value` (already checked to fit the width) as the prefix.
    /// Returns the bytes and how many of them are used.
    #[inline]
    fn prefix_bytes(&self, value: u64) -> ([u8; 8], usize) {
        let n = self.width.header_size();
        // `value` fits in `n` bytes, so after the shift (big endian) or as is
        // (little endian) the prefix occupies exactly the first `n` bytes.
        let out = match self.endian {
            Endian::Big => (value << (8 * (8 - n))).to_be_bytes(),
            Endian::Little => value.to_le_bytes(),
        };
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
    /// - [`Error::FrameTooLarge`] if the (adjusted) prefix exceeds the
    ///   configured limit, or the payload cannot be addressed on this target.
    /// - [`Error::InvalidLengthPrefix`] if the length adjustment turns the
    ///   prefix into a negative length.
    fn next_frame<'a>(&self, input: &'a [u8]) -> Result<Option<Frame<'a>>> {
        let header_size = self.width.header_size();
        let Some(raw) = self.read_prefix(input) else {
            return Ok(None);
        };
        // The prefix is attacker-controlled: the adjustment is applied with
        // checked arithmetic, so neither the sum nor its sign can wrap.
        let len = match raw.checked_add_signed(self.adjustment) {
            Some(len) => len,
            None if self.adjustment < 0 => return Err(Error::InvalidLengthPrefix),
            None => return Err(self.too_large(u64::MAX)),
        };
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
    ///   limit, or its adjusted length does not fit in the prefix width.
    /// - [`Error::InvalidLengthPrefix`] if the adjusted length is negative.
    /// - [`Error::BufferFull`] if `out` cannot hold the whole frame.
    ///
    /// Nothing is written on error.
    fn write_frame(&self, payload: &[u8], out: &mut WriteBuf<'_>) -> Result<()> {
        let len = u64::try_from(payload.len()).unwrap_or(u64::MAX);
        if len > self.max_payload {
            return Err(self.too_large(len));
        }
        // prefix = len - adjustment, in i128 so `-i64::MIN` cannot overflow.
        let prefix = if self.adjustment == 0 {
            len
        } else {
            let prefix = i128::from(len) - i128::from(self.adjustment);
            if prefix < 0 {
                return Err(Error::InvalidLengthPrefix);
            }
            u64::try_from(prefix).unwrap_or(u64::MAX)
        };
        if prefix > self.width.max_payload() {
            // Report the largest payload this width and adjustment can carry.
            let effective = i128::from(self.width.max_payload()) + i128::from(self.adjustment);
            let effective = u64::try_from(effective.max(0)).unwrap_or(u64::MAX);
            return Err(Error::FrameTooLarge {
                len: saturate_usize(len),
                limit: saturate_usize(effective.min(self.max_payload)),
            });
        }
        let (header, header_len) = self.prefix_bytes(prefix);
        let total = payload
            .len()
            .checked_add(header_len)
            .ok_or(Error::BufferFull)?;
        if total > out.remaining() {
            return Err(Error::BufferFull);
        }
        out.write_bytes(&header[..header_len])?;
        out.write_bytes(payload)
    }

    #[inline]
    fn max_frame_len(&self, payload_len: usize) -> Option<usize> {
        payload_len.checked_add(self.width.header_size())
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
        let framer = framer.with_length_adjustment(i64::MAX);
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
                assert_eq!(framer.max_frame_len(3), Some(n));
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
            assert_eq!(framer.read_prefix(header), Some(0x0102));
            assert_eq!(framer.read_prefix(&header[1..]), None);
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
    fn write_is_atomic_on_buffer_full() {
        let framer = LengthPrefixed::new(LengthWidth::U32, Endian::Big);
        let mut out = [0xEEu8; 6];
        let mut buf = WriteBuf::new(&mut out);
        assert_eq!(framer.write_frame(b"abc", &mut buf), Err(Error::BufferFull));
        assert_eq!(buf.position(), 0);
        assert_eq!(out, [0xEE; 6], "no partial header is written");
    }

    #[test]
    fn bounded_constructor() {
        let framer = LengthPrefixed::bounded(LengthWidth::U64, Endian::Big);
        assert_eq!(framer.max_payload(), LengthPrefixed::DEFAULT_MAX_PAYLOAD);
        assert_eq!(framer.width(), LengthWidth::U64);
        assert_eq!(framer.endian(), Endian::Big);
        assert_eq!(framer.length_adjustment(), 0);
        let mut input = [0u8; 8];
        input.copy_from_slice(&(LengthPrefixed::DEFAULT_MAX_PAYLOAD + 1).to_be_bytes());
        assert!(matches!(
            framer.next_frame(&input),
            Err(Error::FrameTooLarge { .. })
        ));
    }

    #[test]
    fn length_adjustment_including_header() {
        let framer = LengthPrefixed::new(LengthWidth::U32, Endian::Big).with_length_adjustment(-4);
        let mut out = [0u8; 16];
        let mut buf = WriteBuf::new(&mut out);
        framer.write_frame(b"xyz", &mut buf).unwrap();
        assert_eq!(buf.written(), &[0, 0, 0, 7, b'x', b'y', b'z']);
        let frame = framer.next_frame(&out[..7]).unwrap().unwrap();
        assert_eq!(frame.payload(), b"xyz");
        assert_eq!(frame.consumed(), 7);
        // A prefix smaller than the header adjusts to a negative length.
        assert_eq!(
            framer.next_frame(&[0, 0, 0, 3]),
            Err(Error::InvalidLengthPrefix)
        );
    }

    #[test]
    fn length_adjustment_positive() {
        // The prefix omits a 2-byte field that is part of the payload.
        let framer = LengthPrefixed::new(LengthWidth::U8, Endian::Big).with_length_adjustment(2);
        let frame = framer
            .next_frame(&[0x01, b'a', b'b', b'c'])
            .unwrap()
            .unwrap();
        assert_eq!(frame.payload(), b"abc");
        let mut out = [0u8; 8];
        let mut buf = WriteBuf::new(&mut out);
        framer.write_frame(b"abc", &mut buf).unwrap();
        assert_eq!(buf.written(), &[0x01, b'a', b'b', b'c']);
        // A payload shorter than the adjustment cannot be represented.
        assert_eq!(
            framer.write_frame(b"a", &mut buf),
            Err(Error::InvalidLengthPrefix)
        );
    }

    #[test]
    fn length_adjustment_overflowing_the_width() {
        // 255 payload bytes plus a self-counting header does not fit in u8.
        let framer = LengthPrefixed::new(LengthWidth::U8, Endian::Big).with_length_adjustment(-1);
        let mut out = [0u8; 512];
        let mut buf = WriteBuf::new(&mut out);
        assert!(matches!(
            framer.write_frame(&[0u8; 255], &mut buf),
            Err(Error::FrameTooLarge { .. })
        ));
        // The error names the effective bound, not the configured 255.
        assert_eq!(
            framer.write_frame(&[0u8; 255], &mut buf),
            Err(Error::FrameTooLarge {
                len: 255,
                limit: 254
            })
        );
        framer.write_frame(&[0u8; 254], &mut buf).unwrap();
        assert_eq!(buf.written()[0], 255);
        // A positive adjustment does not raise the payload limit.
        let framer = LengthPrefixed::new(LengthWidth::U8, Endian::Big).with_length_adjustment(2);
        assert_eq!(framer.max_payload(), 255);
        assert_eq!(
            framer.next_frame(&[0xFF]),
            Err(Error::FrameTooLarge {
                len: 257,
                limit: 255
            })
        );
    }

    #[test]
    fn saturate_usize_is_identity_when_it_fits() {
        assert_eq!(saturate_usize(42), 42);
        if usize::BITS == 32 {
            assert_eq!(saturate_usize(u64::MAX), usize::MAX);
        }
    }
}
