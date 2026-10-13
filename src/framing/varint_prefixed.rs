//! Varint length-prefixed framing.
//!
//! Each frame is a LEB128 varint giving the payload length, followed by the
//! payload. This is the "length-delimited" stream format of Protocol Buffers
//! (`writeDelimitedTo` / `parseDelimitedFrom`, `prost::encode_length_delimited`)
//! and of several RPC transports. Small messages pay a single byte of
//! overhead; the prefix grows by one byte per seven bits of length.

use crate::buf::WriteBuf;
use crate::error::{Error, Result};
use crate::framing::{Frame, Framer};
use crate::varint::{self, MAX_LEN_U64};

/// Varint length-prefixed framer.
///
/// Unlike [`LengthPrefixed::new`][`crate::framing::LengthPrefixed::new`],
/// this framer is bounded by default: [`VarintPrefixed::new`] caps payloads
/// at [`VarintPrefixed::DEFAULT_MAX_PAYLOAD`], and it rejects overlong
/// (non-canonical) prefixes so every frame has exactly one valid encoding.
/// Both can be relaxed.
///
/// # Example
///
/// ```
/// use wire_codec::WriteBuf;
/// use wire_codec::framing::{Framer, VarintPrefixed};
///
/// let framer = VarintPrefixed::new();
///
/// let mut out = [0u8; 16];
/// let mut buf = WriteBuf::new(&mut out);
/// framer.write_frame(b"hello", &mut buf).unwrap();
/// assert_eq!(buf.written(), b"\x05hello");
///
/// let frame = framer.next_frame(&out[..6]).unwrap().unwrap();
/// assert_eq!(frame.payload(), b"hello");
/// assert_eq!(frame.consumed(), 6);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct VarintPrefixed {
    max_payload: usize,
    canonical: bool,
}

impl VarintPrefixed {
    /// Default payload limit: 8 MiB, matching
    /// [`LengthPrefixed::DEFAULT_MAX_PAYLOAD`][`crate::framing::LengthPrefixed::DEFAULT_MAX_PAYLOAD`].
    pub const DEFAULT_MAX_PAYLOAD: usize = 8 * 1024 * 1024;

    /// Build a framer with the default payload limit that rejects
    /// non-canonical prefixes.
    ///
    /// # Example
    ///
    /// ```
    /// use wire_codec::framing::VarintPrefixed;
    ///
    /// let framer = VarintPrefixed::new();
    /// assert_eq!(framer.max_payload(), VarintPrefixed::DEFAULT_MAX_PAYLOAD);
    /// assert!(framer.is_canonical());
    /// ```
    #[inline]
    #[must_use]
    pub const fn new() -> Self {
        Self {
            max_payload: Self::DEFAULT_MAX_PAYLOAD,
            canonical: true,
        }
    }

    /// Set the payload size limit enforced when reading and writing frames.
    ///
    /// # Example
    ///
    /// ```
    /// use wire_codec::framing::{Framer, VarintPrefixed};
    /// use wire_codec::Error;
    ///
    /// let framer = VarintPrefixed::new().with_max_payload(4);
    /// assert!(matches!(
    ///     framer.next_frame(&[0x05, 1, 2, 3, 4, 5]),
    ///     Err(Error::FrameTooLarge { len: 5, limit: 4 })
    /// ));
    /// ```
    #[inline]
    #[must_use]
    pub const fn with_max_payload(mut self, max: usize) -> Self {
        self.max_payload = max;
        self
    }

    /// Choose whether overlong prefixes (such as `[0x85, 0x00]` for length 5)
    /// are rejected with [`Error::InvalidEncoding`] (`true`, the default) or
    /// accepted the way protobuf parsers do (`false`).
    ///
    /// # Example
    ///
    /// ```
    /// use wire_codec::framing::{Framer, VarintPrefixed};
    /// use wire_codec::Error;
    ///
    /// let overlong = [0x81, 0x00, b'x'];
    /// assert_eq!(VarintPrefixed::new().next_frame(&overlong), Err(Error::InvalidEncoding));
    /// let lenient = VarintPrefixed::new().with_canonical(false);
    /// assert_eq!(lenient.next_frame(&overlong).unwrap().unwrap().payload(), b"x");
    /// ```
    #[inline]
    #[must_use]
    pub const fn with_canonical(mut self, canonical: bool) -> Self {
        self.canonical = canonical;
        self
    }

    /// Configured maximum payload size in bytes.
    #[inline]
    #[must_use]
    pub const fn max_payload(&self) -> usize {
        self.max_payload
    }

    /// Whether overlong prefixes are rejected.
    #[inline]
    #[must_use]
    pub const fn is_canonical(&self) -> bool {
        self.canonical
    }

    fn too_large(&self, len: u64) -> Error {
        Error::FrameTooLarge {
            len: usize::try_from(len).unwrap_or(usize::MAX),
            limit: self.max_payload,
        }
    }
}

impl Default for VarintPrefixed {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl Framer for VarintPrefixed {
    /// Extract one frame if the varint prefix and the full payload are present.
    ///
    /// # Errors
    ///
    /// - [`Error::VarintOverflow`] if the prefix runs past ten bytes or
    ///   overflows `u64`.
    /// - [`Error::InvalidEncoding`] if the prefix is overlong and the framer
    ///   is canonical.
    /// - [`Error::FrameTooLarge`] if the length exceeds the configured limit.
    fn next_frame<'a>(&self, input: &'a [u8]) -> Result<Option<Frame<'a>>> {
        let (len, header_size) =
            match varint::decode_slice(input, MAX_LEN_U64, u64::BITS, self.canonical) {
                Ok(parsed) => parsed,
                Err(Error::UnexpectedEof) => return Ok(None),
                Err(e) => return Err(e),
            };
        let consumed = usize::try_from(len)
            .ok()
            .filter(|&len| len <= self.max_payload)
            .and_then(|len| len.checked_add(header_size))
            .ok_or_else(|| self.too_large(len))?;
        let Some(payload) = input.get(header_size..consumed) else {
            return Ok(None);
        };
        Ok(Some(Frame::new(payload, consumed)))
    }

    /// Write the varint prefix and the payload.
    ///
    /// # Errors
    ///
    /// - [`Error::FrameTooLarge`] if the payload exceeds the configured limit.
    /// - [`Error::BufferFull`] if `out` cannot hold the whole frame.
    ///
    /// Nothing is written on error.
    fn write_frame(&self, payload: &[u8], out: &mut WriteBuf<'_>) -> Result<()> {
        if payload.len() > self.max_payload {
            return Err(Error::FrameTooLarge {
                len: payload.len(),
                limit: self.max_payload,
            });
        }
        let total = self.max_frame_len(payload.len()).ok_or(Error::BufferFull)?;
        if total > out.remaining() {
            return Err(Error::BufferFull);
        }
        varint::encode_usize(payload.len(), out)?;
        out.write_bytes(payload)
    }

    #[inline]
    fn max_frame_len(&self, payload_len: usize) -> Option<usize> {
        payload_len.checked_add(varint::encoded_len_usize(payload_len))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_across_prefix_sizes() {
        let framer = VarintPrefixed::new();
        let mut out = vec![0u8; 20_000];
        for len in [0usize, 1, 127, 128, 16_383, 16_384] {
            let payload = vec![0x5Au8; len];
            let mut buf = WriteBuf::new(&mut out);
            framer.write_frame(&payload, &mut buf).unwrap();
            let n = buf.position();
            assert_eq!(Some(n), framer.max_frame_len(len));
            let frame = framer.next_frame(&out[..n]).unwrap().unwrap();
            assert_eq!(frame.payload(), payload.as_slice());
            assert_eq!(frame.consumed(), n);
            // Every strict prefix is incomplete.
            for cut in 0..n {
                assert_eq!(
                    framer.next_frame(&out[..cut]),
                    Ok(None),
                    "len={len} cut={cut}"
                );
            }
        }
    }

    #[test]
    fn limit_boundaries() {
        let framer = VarintPrefixed::new().with_max_payload(3);
        assert_eq!(
            framer.next_frame(&[3, 1, 2, 3]).unwrap().unwrap().payload(),
            &[1, 2, 3]
        );
        assert_eq!(
            framer.next_frame(&[4]),
            Err(Error::FrameTooLarge { len: 4, limit: 3 })
        );
        let mut out = [0u8; 8];
        let mut buf = WriteBuf::new(&mut out);
        assert_eq!(
            framer.write_frame(&[0; 4], &mut buf),
            Err(Error::FrameTooLarge { len: 4, limit: 3 })
        );
        assert_eq!(buf.position(), 0);
    }

    #[test]
    fn hostile_prefixes() {
        let framer = VarintPrefixed::new().with_max_payload(usize::MAX);
        // Eleven continuation bytes: overflow, not "need more".
        assert_eq!(framer.next_frame(&[0x80; 11]), Err(Error::VarintOverflow));
        assert_eq!(framer.next_frame(&[0x80; 10]), Err(Error::VarintOverflow));
        // Nine continuation bytes: still a possible prefix.
        assert_eq!(framer.next_frame(&[0x80; 9]), Ok(None));
        // A u64::MAX length cannot be addressed even with no limit set:
        // the header-size addition overflows on 64-bit targets and the
        // conversion to usize fails on 32-bit ones. Never a panic.
        let max = [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x01];
        assert!(matches!(
            framer.next_frame(&max),
            Err(Error::FrameTooLarge { .. })
        ));
    }

    #[test]
    fn write_is_atomic_on_buffer_full() {
        let framer = VarintPrefixed::new();
        let mut out = [0xEEu8; 3];
        let mut buf = WriteBuf::new(&mut out);
        assert_eq!(framer.write_frame(b"abc", &mut buf), Err(Error::BufferFull));
        assert_eq!(out, [0xEE; 3]);
    }

    #[test]
    fn default_matches_new() {
        assert_eq!(VarintPrefixed::default(), VarintPrefixed::new());
        assert_eq!(VarintPrefixed::new().max_frame_len(usize::MAX), None);
    }
}
