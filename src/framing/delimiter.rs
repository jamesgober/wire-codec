//! Byte-delimited framing.
//!
//! Frames are separated by a configured delimiter byte sequence (typically
//! `\n` for newline-terminated text, or `\r\n` for HTTP-style protocols). The
//! delimiter is stripped from the emitted payload but counted toward
//! [`Frame::consumed`].
//!
//! # Limits and streaming
//!
//! [`Delimited::new`] keeps the 1.0 behaviour of an unbounded payload size,
//! which lets a peer that never sends the delimiter grow the caller's buffer
//! without limit. For untrusted input, use [`Delimited::bounded`] (a
//! [`Delimited::DEFAULT_MAX_PAYLOAD`] cap) or [`Delimited::with_max_payload`].
//!
//! When input arrives in pieces, call [`Framer::next_frame_from`] or use a
//! [`StreamDecoder`][`crate::framing::StreamDecoder`] so the search resumes
//! where it stopped instead of rescanning the buffered prefix on every call.

use crate::buf::WriteBuf;
use crate::error::{Error, Result};
use crate::framing::search;
use crate::framing::{Frame, Framer};

/// Byte-delimited framer.
///
/// # Example
///
/// ```
/// use wire_codec::WriteBuf;
/// use wire_codec::framing::{Delimited, Framer};
///
/// let framer = Delimited::bounded(b"\n").unwrap();
///
/// let mut out = [0u8; 32];
/// let mut buf = WriteBuf::new(&mut out);
/// framer.write_frame(b"line one", &mut buf).unwrap();
/// framer.write_frame(b"line two", &mut buf).unwrap();
/// let n = buf.position();
/// assert_eq!(&out[..n], b"line one\nline two\n");
///
/// let first = framer.next_frame(&out[..n]).unwrap().unwrap();
/// assert_eq!(first.payload(), b"line one");
/// assert_eq!(first.consumed(), 9);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Delimited<'d> {
    delimiter: &'d [u8],
    max_payload: usize,
}

impl<'d> Delimited<'d> {
    /// Payload limit applied by [`Delimited::bounded`]: 64 KiB.
    ///
    /// Generous for line-oriented text protocols (SMTP caps a line at 1000
    /// bytes, HTTP servers commonly cap a header line at 8 KiB) while
    /// bounding the memory a single peer can pin.
    pub const DEFAULT_MAX_PAYLOAD: usize = 64 * 1024;

    /// Build a framer using `delimiter` to separate frames, with no payload
    /// size limit.
    ///
    /// Without a limit the framer keeps asking for more input until it sees
    /// the delimiter, however long that takes. Prefer [`Delimited::bounded`]
    /// for input from untrusted peers.
    ///
    /// # Errors
    ///
    /// Returns [`Error::EmptyDelimiter`] if `delimiter` is empty. An empty
    /// delimiter cannot uniquely separate frames.
    ///
    /// # Example
    ///
    /// ```
    /// use wire_codec::framing::Delimited;
    ///
    /// let framer = Delimited::new(b"\r\n").unwrap();
    /// assert_eq!(framer.max_payload(), usize::MAX);
    /// ```
    #[inline]
    pub const fn new(delimiter: &'d [u8]) -> Result<Self> {
        if delimiter.is_empty() {
            return Err(Error::EmptyDelimiter);
        }
        Ok(Self {
            delimiter,
            max_payload: usize::MAX,
        })
    }

    /// Build a framer using `delimiter`, capped at
    /// [`Delimited::DEFAULT_MAX_PAYLOAD`] payload bytes per frame.
    ///
    /// This is the recommended constructor for input from untrusted peers.
    /// Chain [`Delimited::with_max_payload`] to pick a different cap.
    ///
    /// # Errors
    ///
    /// Returns [`Error::EmptyDelimiter`] if `delimiter` is empty.
    ///
    /// # Example
    ///
    /// ```
    /// use wire_codec::framing::Delimited;
    ///
    /// let framer = Delimited::bounded(b"\n").unwrap();
    /// assert_eq!(framer.max_payload(), Delimited::DEFAULT_MAX_PAYLOAD);
    /// ```
    #[inline]
    pub const fn bounded(delimiter: &'d [u8]) -> Result<Self> {
        match Self::new(delimiter) {
            Ok(framer) => Ok(framer.with_max_payload(Self::DEFAULT_MAX_PAYLOAD)),
            Err(e) => Err(e),
        }
    }

    /// Set an upper bound on payload size. Frames whose payload would exceed
    /// this size cause [`Error::FrameTooLarge`].
    ///
    /// The delimiter search never looks further than `max + delimiter.len()`
    /// bytes into the input, so the limit also bounds the work per call.
    ///
    /// # Example
    ///
    /// ```
    /// use wire_codec::framing::{Delimited, Framer};
    /// use wire_codec::Error;
    ///
    /// let framer = Delimited::new(b"\n").unwrap().with_max_payload(4);
    /// assert!(framer.next_frame(b"1234\n").unwrap().is_some());
    /// assert!(matches!(
    ///     framer.next_frame(b"123456"),
    ///     Err(Error::FrameTooLarge { limit: 4, .. })
    /// ));
    /// ```
    #[inline]
    #[must_use]
    pub const fn with_max_payload(mut self, max: usize) -> Self {
        self.max_payload = max;
        self
    }

    /// Configured delimiter sequence.
    #[inline]
    pub const fn delimiter(&self) -> &'d [u8] {
        self.delimiter
    }

    /// Configured maximum payload size.
    #[inline]
    pub const fn max_payload(&self) -> usize {
        self.max_payload
    }
}

impl Framer for Delimited<'_> {
    /// Extract the bytes up to the first delimiter.
    ///
    /// # Errors
    ///
    /// Returns [`Error::FrameTooLarge`] once more than
    /// `max_payload + delimiter.len()` bytes are buffered without a
    /// delimiter starting at or before offset `max_payload`. The `len` field
    /// is then a lower bound (`max_payload + 1`): the true length is unknown
    /// because the frame has not ended yet.
    #[inline]
    fn next_frame<'a>(&self, input: &'a [u8]) -> Result<Option<Frame<'a>>> {
        self.next_frame_from(input, &mut 0)
    }

    /// Serialize `payload` followed by the delimiter.
    ///
    /// # Errors
    ///
    /// - [`Error::FrameTooLarge`] if `payload` exceeds the configured limit.
    /// - [`Error::DelimiterInPayload`] if `payload` contains the delimiter,
    ///   because the receiver would split it into more than one frame.
    ///   Nothing is written in that case.
    /// - [`Error::BufferFull`] if `out` cannot hold the payload and the
    ///   delimiter.
    fn write_frame(&self, payload: &[u8], out: &mut WriteBuf<'_>) -> Result<()> {
        if payload.len() > self.max_payload {
            return Err(Error::FrameTooLarge {
                len: payload.len(),
                limit: self.max_payload,
            });
        }
        if search::find(payload, self.delimiter, 0).is_some() {
            return Err(Error::DelimiterInPayload);
        }
        out.write_bytes(payload)?;
        out.write_bytes(self.delimiter)
    }

    /// Resumable delimiter search; see [`Framer::next_frame_from`].
    ///
    /// On `Ok(None)`, `scanned` is set to the number of leading bytes that
    /// can no longer start a delimiter, so the next call only examines new
    /// input (plus `delimiter.len() - 1` bytes of overlap for a delimiter
    /// split across reads). Values larger than the input are clamped.
    ///
    /// # Errors
    ///
    /// Same contract as [`Framer::next_frame`] for this type.
    fn next_frame_from<'a>(
        &self,
        input: &'a [u8],
        scanned: &mut usize,
    ) -> Result<Option<Frame<'a>>> {
        let delim_len = self.delimiter.len();
        // A valid delimiter starts at offset <= max_payload, so it ends within
        // `max_payload + delim_len` bytes. Saturating because the unbounded
        // default is usize::MAX.
        let window_cap = self.max_payload.saturating_add(delim_len);
        let window = if input.len() > window_cap {
            &input[..window_cap]
        } else {
            input
        };
        let start = (*scanned).min(window.len());
        if let Some(pos) = search::find(window, self.delimiter, start) {
            *scanned = 0;
            return Ok(Some(Frame::new(&input[..pos], pos + delim_len)));
        }
        if input.len() > window_cap {
            // No delimiter inside the window and the input has already run
            // past it: the payload is at least max_payload + 1 bytes long.
            return Err(Error::FrameTooLarge {
                len: self.max_payload.saturating_add(1),
                limit: self.max_payload,
            });
        }
        // Every offset before this one has been ruled out as a delimiter
        // start. The last `delim_len - 1` bytes might begin a delimiter that
        // completes once more input arrives.
        *scanned = window.len().saturating_sub(delim_len - 1);
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::framing::StreamDecoder;

    #[test]
    fn newline_round_trip() {
        let framer = Delimited::new(b"\n").unwrap();
        let mut storage = [0u8; 32];
        let mut buf = WriteBuf::new(&mut storage);
        framer.write_frame(b"hello", &mut buf).unwrap();
        let n = buf.position();
        assert_eq!(&storage[..n], b"hello\n");

        let frame = framer.next_frame(&storage[..n]).unwrap().unwrap();
        assert_eq!(frame.payload(), b"hello");
        assert_eq!(frame.consumed(), 6);
    }

    #[test]
    fn multi_byte_delimiter() {
        let framer = Delimited::new(b"\r\n").unwrap();
        let frame = framer.next_frame(b"GET /\r\nrest").unwrap().unwrap();
        assert_eq!(frame.payload(), b"GET /");
        assert_eq!(frame.consumed(), 7);
    }

    #[test]
    fn overlapping_delimiter_prefix() {
        let framer = Delimited::new(b"\r\n").unwrap();
        let frame = framer.next_frame(b"ab\r\r\ncd").unwrap().unwrap();
        assert_eq!(frame.payload(), b"ab\r");
        assert_eq!(frame.consumed(), 5);
    }

    #[test]
    fn no_delimiter_returns_none() {
        let framer = Delimited::new(b"\n").unwrap();
        assert_eq!(framer.next_frame(b"no terminator here").unwrap(), None);
    }

    #[test]
    fn exceeds_max_payload_short_circuits_search() {
        // Input is 6 bytes; max_payload=3 limits the scan window to 4 bytes
        // (3 + delimiter length). The delimiter is beyond the window, so the
        // framer bails with FrameTooLarge instead of scanning to the end.
        let framer = Delimited::new(b"\n").unwrap().with_max_payload(3);
        let result = framer.next_frame(b"abcde\n");
        assert_eq!(result, Err(Error::FrameTooLarge { len: 4, limit: 3 }));
    }

    #[test]
    fn read_limit_boundaries() {
        let framer = Delimited::new(b"\r\n").unwrap().with_max_payload(3);
        // Exactly max_payload bytes of payload is accepted.
        let frame = framer.next_frame(b"abc\r\n").unwrap().unwrap();
        assert_eq!(frame.payload(), b"abc");
        // max_payload + delimiter bytes without a delimiter: still waiting,
        // because the delimiter could start at offset 3.
        assert_eq!(framer.next_frame(b"abc\r"), Ok(None));
        assert_eq!(framer.next_frame(b"abcd\r"), Ok(None));
        // One byte past the window without a delimiter: too large.
        assert_eq!(
            framer.next_frame(b"abcd\r\n"),
            Err(Error::FrameTooLarge { len: 4, limit: 3 })
        );
    }

    #[test]
    fn write_limit_boundaries() {
        let framer = Delimited::new(b"\n").unwrap().with_max_payload(3);
        let mut storage = [0u8; 16];
        let mut buf = WriteBuf::new(&mut storage);
        framer.write_frame(b"abc", &mut buf).unwrap();
        assert_eq!(
            framer.write_frame(b"abcd", &mut buf),
            Err(Error::FrameTooLarge { len: 4, limit: 3 })
        );
        assert_eq!(buf.written(), b"abc\n");
    }

    #[test]
    fn write_rejects_delimiter_inside_payload() {
        let framer = Delimited::new(b"\r\n").unwrap();
        let mut storage = [0u8; 16];
        let mut buf = WriteBuf::new(&mut storage);
        assert_eq!(
            framer.write_frame(b"a\r\nb", &mut buf),
            Err(Error::DelimiterInPayload)
        );
        assert_eq!(buf.position(), 0, "nothing is written on error");
        // A lone '\r' or '\n' is not the delimiter.
        framer.write_frame(b"a\rb\nc", &mut buf).unwrap();
        let n = buf.position();
        let frame = framer.next_frame(&storage[..n]).unwrap().unwrap();
        assert_eq!(frame.payload(), b"a\rb\nc");
        assert_eq!(frame.consumed(), n);
    }

    #[test]
    fn empty_payload_is_valid() {
        let framer = Delimited::new(b"\n").unwrap();
        let frame = framer.next_frame(b"\nrest").unwrap().unwrap();
        assert_eq!(frame.payload(), b"");
        assert_eq!(frame.consumed(), 1);
    }

    #[test]
    fn empty_delimiter_rejected() {
        assert!(matches!(Delimited::new(b""), Err(Error::EmptyDelimiter)));
        assert!(matches!(
            Delimited::bounded(b""),
            Err(Error::EmptyDelimiter)
        ));
    }

    #[test]
    fn bounded_constructor() {
        let framer = Delimited::bounded(b"\r\n").unwrap();
        assert_eq!(framer.max_payload(), Delimited::DEFAULT_MAX_PAYLOAD);
    }

    #[test]
    fn resumable_scan_handles_split_delimiter() {
        let framer = Delimited::new(b"\r\n").unwrap();
        let mut scanned = 0;
        assert_eq!(framer.next_frame_from(b"abc\r", &mut scanned), Ok(None));
        // The trailing '\r' may begin the delimiter, so it is not skipped.
        assert_eq!(scanned, 3);
        let frame = framer
            .next_frame_from(b"abc\r\n", &mut scanned)
            .unwrap()
            .unwrap();
        assert_eq!(frame.payload(), b"abc");
        assert_eq!(scanned, 0);
    }

    #[test]
    fn resumable_scan_clamps_oversized_hint() {
        let framer = Delimited::new(b"\n").unwrap();
        let mut scanned = 1_000;
        // The hint is a promise about the input; an out-of-range value is
        // clamped rather than trusted.
        assert_eq!(framer.next_frame_from(b"ab", &mut scanned), Ok(None));
        assert_eq!(scanned, 2);
    }

    #[test]
    fn byte_at_a_time_stream_does_linear_work() {
        // A 256 KiB line delivered one byte at a time. With the resumable
        // scan each call only looks at the new byte, so the total number of
        // bytes examined is bounded by the input length plus overlap.
        let framer = Delimited::new(b"\r\n").unwrap();
        let mut decoder = StreamDecoder::new(framer);
        let mut line = vec![b'x'; 256 * 1024];
        line.extend_from_slice(b"\r\n");
        let mut examined = 0usize;
        let mut frames = 0;
        for end in 1..=line.len() {
            let before = decoder.scanned;
            match decoder.decode(&line[..end]).unwrap() {
                Some(frame) => {
                    assert_eq!(frame.payload().len(), 256 * 1024);
                    frames += 1;
                }
                None => examined += end - before,
            }
        }
        assert_eq!(frames, 1);
        assert!(examined <= 2 * line.len(), "examined {examined} bytes");
    }
}
