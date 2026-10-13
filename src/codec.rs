//! Adapters that run a [`Framer`] over a growable [`bytes::BytesMut`] buffer.
//!
//! Available with the `bytes` feature. [`FrameCodec`] wraps any framer and
//! exposes two inherent methods, [`FrameCodec::decode_frame`] and
//! [`FrameCodec::encode_frame`], that work on `BytesMut` directly. The
//! `tokio-util` and `asynchronous-codec` features add implementations of
//! those crates' `Decoder` and `Encoder` traits on top, so a framer plugs
//! straight into `tokio_util::codec::Framed` or `asynchronous_codec::Framed`:
//!
//! ```ignore
//! use tokio_util::codec::Framed;
//! use wire_codec::codec::FrameCodec;
//! use wire_codec::framing::{Endian, LengthPrefixed, LengthWidth};
//!
//! let codec = FrameCodec::new(LengthPrefixed::bounded(LengthWidth::U32, Endian::Big));
//! let mut framed = Framed::new(tcp_stream, codec);
//! // framed: Stream<Item = io::Result<BytesMut>> + Sink<&[u8]>
//! ```
//!
//! Decoded payloads are split off the receive buffer without copying when
//! the framer returns a payload that lies inside the frame (all built-in
//! framers do). The decoder keeps the resumable-scan state described on
//! [`Framer::next_frame_from`], so a slow peer cannot force quadratic work.
//!
//! The trait implementations use [`std::io::Error`] as their error type,
//! like tokio-util's own codecs; the original [`Error`] is available through
//! `io::Error::get_ref` and `downcast_ref`.

use bytes::{Buf, BytesMut};

use crate::buf::WriteBuf;
use crate::error::{Error, Result};
use crate::framing::{Framer, StreamDecoder};

/// A [`Framer`] driven over a `BytesMut` buffer.
///
/// # Example
///
/// ```
/// use bytes::BytesMut;
/// use wire_codec::codec::FrameCodec;
/// use wire_codec::framing::VarintPrefixed;
///
/// let mut codec = FrameCodec::new(VarintPrefixed::new());
/// let mut wire = BytesMut::new();
/// codec.encode_frame(b"hello", &mut wire).unwrap();
/// codec.encode_frame(b"world", &mut wire).unwrap();
///
/// assert_eq!(&codec.decode_frame(&mut wire).unwrap().unwrap()[..], b"hello");
/// assert_eq!(&codec.decode_frame(&mut wire).unwrap().unwrap()[..], b"world");
/// assert_eq!(codec.decode_frame(&mut wire).unwrap(), None);
/// assert!(wire.is_empty());
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct FrameCodec<F> {
    decoder: StreamDecoder<F>,
}

impl<F: Framer> FrameCodec<F> {
    /// Wrap `framer`.
    #[inline]
    #[must_use]
    pub const fn new(framer: F) -> Self {
        Self {
            decoder: StreamDecoder::new(framer),
        }
    }

    /// Borrow the wrapped framer.
    #[inline]
    #[must_use]
    pub const fn framer(&self) -> &F {
        self.decoder.framer()
    }

    /// Consume the codec and return the wrapped framer.
    #[inline]
    #[must_use]
    pub fn into_inner(self) -> F {
        self.decoder.into_inner()
    }

    /// Forget the partial-scan progress kept between
    /// [`FrameCodec::decode_frame`] calls.
    ///
    /// The codec assumes the receive buffer only grows at the back between
    /// calls, apart from the frames it splits off itself. Call `reset` when
    /// you clear, truncate, or replace the buffer (for example to
    /// resynchronise, after `Framed::from_parts` with a fresh read buffer, or
    /// when reusing the codec for a new connection). Progress is also cleared
    /// automatically whenever decoding returns an error.
    ///
    /// # Example
    ///
    /// ```
    /// use bytes::BytesMut;
    /// use wire_codec::codec::FrameCodec;
    /// use wire_codec::framing::Delimited;
    ///
    /// let mut codec = FrameCodec::new(Delimited::bounded(b"\n").unwrap());
    /// let mut src = BytesMut::from(&b"partial line"[..]);
    /// assert_eq!(codec.decode_frame(&mut src).unwrap(), None);
    ///
    /// // Switch to a different stream: drop the buffer and the progress.
    /// src.clear();
    /// codec.reset();
    /// src.extend_from_slice(b"a\nb\n");
    /// assert_eq!(&codec.decode_frame(&mut src).unwrap().unwrap()[..], b"a");
    /// ```
    #[inline]
    pub fn reset(&mut self) {
        self.decoder.reset();
    }

    /// Remove the next complete frame from the front of `src` and return its
    /// payload, or `Ok(None)` if `src` does not hold a full frame yet.
    ///
    /// # Errors
    ///
    /// Propagates the framer's errors. Returns [`Error::InvalidEncoding`] if
    /// a custom framer violates the [`Frame`][`crate::framing::Frame`]
    /// contract (zero bytes consumed, or more than the input). `src` is left
    /// untouched on error, and the scan progress is cleared so the caller
    /// may discard the buffer and resynchronise.
    pub fn decode_frame(&mut self, src: &mut BytesMut) -> Result<Option<BytesMut>> {
        let (consumed, payload_at) = match self.decoder.decode(src)? {
            None => return Ok(None),
            Some(frame) => {
                let consumed = frame.consumed();
                if consumed == 0 || consumed > src.len() {
                    return Err(Error::InvalidEncoding);
                }
                let payload = frame.payload();
                let start = payload.as_ptr().addr().wrapping_sub(src.as_ptr().addr());
                if start <= consumed && payload.len() <= consumed - start {
                    (consumed, Ok((start, payload.len())))
                } else {
                    // The payload does not live inside the frame (a custom
                    // framer may return a static slice): copy it out.
                    (consumed, Err(BytesMut::from(payload)))
                }
            }
        };
        match payload_at {
            Ok((start, len)) => {
                let mut frame = src.split_to(consumed);
                frame.advance(start);
                frame.truncate(len);
                Ok(Some(frame))
            }
            Err(copied) => {
                src.advance(consumed);
                Ok(Some(copied))
            }
        }
    }

    /// Like [`FrameCodec::decode_frame`], for the end of the stream: once no
    /// more input will arrive, leftover bytes that do not form a frame are an
    /// error.
    ///
    /// # Errors
    ///
    /// Everything [`FrameCodec::decode_frame`] returns, plus
    /// [`Error::UnexpectedEof`] when `src` holds a partial frame.
    pub fn decode_frame_eof(&mut self, src: &mut BytesMut) -> Result<Option<BytesMut>> {
        match self.decode_frame(src)? {
            Some(frame) => Ok(Some(frame)),
            None if src.is_empty() => Ok(None),
            None => Err(Error::UnexpectedEof),
        }
    }

    /// Append `payload` as one frame to `dst`, growing it as needed.
    ///
    /// Space is reserved in one step using [`Framer::max_frame_len`]. For a
    /// custom framer that does not implement it, the codec retries with a
    /// doubling scratch region, up to four times the payload size plus
    /// 4 KiB.
    ///
    /// # Errors
    ///
    /// Propagates the framer's errors, or returns [`Error::BufferFull`] if a
    /// custom framer needs more room than the retry limit. `dst` is left
    /// untouched on error.
    pub fn encode_frame(&mut self, payload: &[u8], dst: &mut BytesMut) -> Result<()> {
        let start = dst.len();
        let framer = self.decoder.framer();
        if let Some(max) = framer.max_frame_len(payload.len()) {
            return write_into(framer, payload, dst, start, max);
        }
        let limit = payload.len().saturating_mul(4).saturating_add(4096);
        let mut room = payload.len().saturating_add(64);
        loop {
            match write_into(framer, payload, dst, start, room) {
                Err(Error::BufferFull) if room < limit => room = room.saturating_mul(2).min(limit),
                result => return result,
            }
        }
    }
}

/// Write one frame into `room` freshly zeroed bytes at the end of `dst`,
/// then trim `dst` to what was written (or back to `start` on error).
fn write_into<F: Framer>(
    framer: &F,
    payload: &[u8],
    dst: &mut BytesMut,
    start: usize,
    room: usize,
) -> Result<()> {
    let end = start.checked_add(room).ok_or(Error::BufferFull)?;
    dst.resize(end, 0);
    let mut out = WriteBuf::new(&mut dst[start..]);
    let result = framer.write_frame(payload, &mut out);
    let written = out.position();
    match result {
        Ok(()) => {
            dst.truncate(start + written);
            Ok(())
        }
        Err(e) => {
            dst.truncate(start);
            Err(e)
        }
    }
}

#[cfg(feature = "tokio-util")]
mod tokio_impls {
    use std::io;

    use bytes::{Bytes, BytesMut};
    use tokio_util::codec::{Decoder, Encoder};

    use super::FrameCodec;
    use crate::framing::Framer;

    #[cfg_attr(docsrs, doc(cfg(feature = "tokio-util")))]
    impl<F: Framer> Decoder for FrameCodec<F> {
        type Item = BytesMut;
        type Error = io::Error;

        #[inline]
        fn decode(&mut self, src: &mut BytesMut) -> io::Result<Option<BytesMut>> {
            self.decode_frame(src).map_err(io::Error::from)
        }

        #[inline]
        fn decode_eof(&mut self, src: &mut BytesMut) -> io::Result<Option<BytesMut>> {
            self.decode_frame_eof(src).map_err(io::Error::from)
        }
    }

    #[cfg_attr(docsrs, doc(cfg(feature = "tokio-util")))]
    impl<F: Framer> Encoder<&[u8]> for FrameCodec<F> {
        type Error = io::Error;

        #[inline]
        fn encode(&mut self, item: &[u8], dst: &mut BytesMut) -> io::Result<()> {
            self.encode_frame(item, dst).map_err(io::Error::from)
        }
    }

    #[cfg_attr(docsrs, doc(cfg(feature = "tokio-util")))]
    impl<F: Framer> Encoder<Bytes> for FrameCodec<F> {
        type Error = io::Error;

        #[inline]
        fn encode(&mut self, item: Bytes, dst: &mut BytesMut) -> io::Result<()> {
            self.encode_frame(&item, dst).map_err(io::Error::from)
        }
    }

    #[cfg_attr(docsrs, doc(cfg(feature = "tokio-util")))]
    impl<F: Framer> Encoder<BytesMut> for FrameCodec<F> {
        type Error = io::Error;

        #[inline]
        fn encode(&mut self, item: BytesMut, dst: &mut BytesMut) -> io::Result<()> {
            self.encode_frame(&item, dst).map_err(io::Error::from)
        }
    }
}

#[cfg(feature = "asynchronous-codec")]
mod futures_impls {
    use std::io;

    use asynchronous_codec::{Decoder, Encoder};
    use bytes::BytesMut;

    use super::FrameCodec;
    use crate::framing::Framer;

    #[cfg_attr(docsrs, doc(cfg(feature = "asynchronous-codec")))]
    impl<F: Framer> Decoder for FrameCodec<F> {
        type Item = BytesMut;
        type Error = io::Error;

        #[inline]
        fn decode(&mut self, src: &mut BytesMut) -> io::Result<Option<BytesMut>> {
            self.decode_frame(src).map_err(io::Error::from)
        }

        #[inline]
        fn decode_eof(&mut self, src: &mut BytesMut) -> io::Result<Option<BytesMut>> {
            self.decode_frame_eof(src).map_err(io::Error::from)
        }
    }

    #[cfg_attr(docsrs, doc(cfg(feature = "asynchronous-codec")))]
    impl<F: Framer> Encoder for FrameCodec<F> {
        type Item<'a> = &'a [u8];
        type Error = io::Error;

        #[inline]
        fn encode(&mut self, item: &[u8], dst: &mut BytesMut) -> io::Result<()> {
            self.encode_frame(item, dst).map_err(io::Error::from)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framing::{Delimited, Endian, Frame, LengthPrefixed, LengthWidth};

    /// Custom framer without `max_frame_len` whose payload is a static
    /// slice, to exercise the fallback paths.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct Fixed {
        overhead: usize,
        consumed_override: Option<usize>,
    }

    impl Framer for Fixed {
        fn next_frame<'a>(&self, input: &'a [u8]) -> Result<Option<Frame<'a>>> {
            if let Some(consumed) = self.consumed_override {
                return Ok(Some(Frame::new(b"", consumed)));
            }
            Ok(input.get(..2).map(|_| Frame::new(b"st", 2)))
        }
        fn write_frame(&self, payload: &[u8], out: &mut WriteBuf<'_>) -> Result<()> {
            if out.remaining() < payload.len() + self.overhead {
                return Err(Error::BufferFull);
            }
            for _ in 0..self.overhead {
                out.write_u8(0xAA)?;
            }
            out.write_bytes(payload)
        }
    }

    #[test]
    fn zero_copy_split_for_builtin_framers() {
        let mut codec = FrameCodec::new(LengthPrefixed::bounded(LengthWidth::U16, Endian::Big));
        let mut wire = BytesMut::new();
        codec.encode_frame(b"abc", &mut wire).unwrap();
        assert_eq!(&wire[..], &[0, 3, b'a', b'b', b'c']);
        wire.extend_from_slice(&[0, 2, b'x']);
        let first = codec.decode_frame(&mut wire).unwrap().unwrap();
        assert_eq!(&first[..], b"abc");
        assert_eq!(codec.decode_frame(&mut wire).unwrap(), None);
        assert_eq!(codec.decode_frame_eof(&mut wire), Err(Error::UnexpectedEof));
        wire.extend_from_slice(b"y");
        assert_eq!(
            &codec.decode_frame_eof(&mut wire).unwrap().unwrap()[..],
            b"xy"
        );
        assert_eq!(codec.decode_frame_eof(&mut wire), Ok(None));
    }

    #[test]
    fn byte_at_a_time_delimited() {
        let mut codec = FrameCodec::new(Delimited::bounded(b"\r\n").unwrap());
        let mut wire = BytesMut::new();
        let mut lines = Vec::new();
        for &byte in b"GET /\r\nHost: x\r\n\r\n" {
            wire.extend_from_slice(&[byte]);
            while let Some(line) = codec.decode_frame(&mut wire).unwrap() {
                lines.push(line.to_vec());
            }
        }
        assert_eq!(lines, [b"GET /".to_vec(), b"Host: x".to_vec(), Vec::new()]);
        assert_eq!(codec.framer().delimiter(), b"\r\n");
    }

    #[test]
    fn resynchronising_after_an_error_does_not_merge_lines() {
        // Regression test for WC2-001, the verifier's scenario: an over-long
        // line, then the caller clears the buffer and feeds fresh lines.
        let framer = Delimited::new(b"\n").unwrap().with_max_payload(8);
        let mut codec = FrameCodec::new(framer);
        let mut src = BytesMut::from(&b"aaaaaaa"[..]);
        assert_eq!(codec.decode_frame(&mut src).unwrap(), None);
        src.extend_from_slice(b"aaaaa");
        assert!(codec.decode_frame(&mut src).is_err());
        src.clear();
        src.extend_from_slice(b"hi\nthere\n");
        assert_eq!(&codec.decode_frame(&mut src).unwrap().unwrap()[..], b"hi");
        assert_eq!(
            &codec.decode_frame(&mut src).unwrap().unwrap()[..],
            b"there"
        );
        assert!(src.is_empty());
    }

    #[test]
    fn reset_after_swapping_the_buffer() {
        let mut codec = FrameCodec::new(Delimited::bounded(b"\n").unwrap());
        let mut old = BytesMut::from(&b"0123456789"[..]);
        assert_eq!(codec.decode_frame(&mut old).unwrap(), None);
        codec.reset();
        let mut fresh = BytesMut::from(&b"x\nyyyyyy\n"[..]);
        assert_eq!(&codec.decode_frame(&mut fresh).unwrap().unwrap()[..], b"x");
        assert_eq!(
            &codec.decode_frame(&mut fresh).unwrap().unwrap()[..],
            b"yyyyyy"
        );
    }

    #[test]
    fn errors_leave_buffers_untouched() {
        let mut codec = FrameCodec::new(Delimited::new(b"\n").unwrap().with_max_payload(3));
        let mut dst = BytesMut::from(&b"keep"[..]);
        assert_eq!(
            codec.encode_frame(b"a\nb", &mut dst),
            Err(Error::DelimiterInPayload)
        );
        assert_eq!(
            codec.encode_frame(b"abcd", &mut dst),
            Err(Error::FrameTooLarge { len: 4, limit: 3 })
        );
        assert_eq!(&dst[..], b"keep");
        let mut src = BytesMut::from(&b"abcde"[..]);
        assert!(codec.decode_frame(&mut src).is_err());
        assert_eq!(&src[..], b"abcde");
        let framer = codec.into_inner();
        assert_eq!(framer.max_payload(), 3);
    }

    #[test]
    fn custom_framer_fallbacks() {
        let mut codec = FrameCodec::new(Fixed {
            overhead: 100,
            consumed_override: None,
        });
        let mut dst = BytesMut::new();
        // No max_frame_len: the scratch region grows until the frame fits.
        codec.encode_frame(b"p", &mut dst).unwrap();
        assert_eq!(dst.len(), 101);
        // Payload outside the input: copied instead of split.
        let mut src = BytesMut::from(&b"zzz"[..]);
        assert_eq!(&codec.decode_frame(&mut src).unwrap().unwrap()[..], b"st");
        assert_eq!(&src[..], b"z");

        // A framer that needs more than the retry limit gets BufferFull.
        let mut codec = FrameCodec::new(Fixed {
            overhead: 10_000,
            consumed_override: None,
        });
        let mut dst = BytesMut::new();
        assert_eq!(codec.encode_frame(b"p", &mut dst), Err(Error::BufferFull));
        assert!(dst.is_empty());
    }

    #[test]
    fn contract_violations_are_reported() {
        for consumed in [0, 99] {
            let mut codec = FrameCodec::new(Fixed {
                overhead: 0,
                consumed_override: Some(consumed),
            });
            let mut src = BytesMut::from(&b"abc"[..]);
            assert_eq!(codec.decode_frame(&mut src), Err(Error::InvalidEncoding));
            assert_eq!(src.len(), 3);
        }
    }
}
