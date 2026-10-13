//! Frame extraction strategies.
//!
//! A [`Framer`] is a stateless protocol that locates and emits one frame at a
//! time from a borrowed byte slice. Frames are described by [`Frame`], which
//! carries a zero-copy reference to the payload alongside the count of bytes
//! the framer consumed from the input.
//!
//! Concrete framers shipped with the crate:
//!
//! - [`length::LengthPrefixed`] for fixed-width length-prefixed protocols.
//! - [`delimiter::Delimited`] for byte-delimited protocols such as newline-
//!   terminated text.
//!
//! Custom framers are implemented by writing a `Framer` impl on a user-defined
//! type.
//!
//! # Untrusted input
//!
//! A framer only guards memory if it has a limit. Use
//! [`Delimited::bounded`] or `with_max_payload` for any stream that comes
//! from a peer you do not control.
//!
//! When bytes arrive in pieces, drive the framer through a [`StreamDecoder`]
//! (or [`Framer::next_frame_from`]) so that a delimiter search resumes where
//! the previous call stopped instead of rescanning the whole buffer. That
//! keeps the total work linear in the input size even when a peer drips one
//! byte at a time.

pub mod delimiter;
pub mod length;
mod search;

pub use delimiter::Delimited;
pub use length::{Endian, LengthPrefixed, LengthWidth};

use crate::buf::WriteBuf;
use crate::error::Result;

/// A frame extracted from an input byte slice.
///
/// The `payload` borrow lives as long as the input slice, so framers never
/// copy data. `consumed` tells the caller how many bytes from the input the
/// frame occupied in total, including any header or delimiter bytes.
///
/// # Example
///
/// ```
/// use wire_codec::framing::Frame;
///
/// let input = b"\x03abcrest";
/// let frame = Frame::new(&input[1..4], 4);
/// assert_eq!(frame.payload(), b"abc");
/// assert_eq!(frame.consumed(), 4);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Frame<'a> {
    payload: &'a [u8],
    consumed: usize,
}

impl<'a> Frame<'a> {
    /// Construct a frame from its payload and total consumed-byte count.
    ///
    /// `consumed` must be at least `payload.len()` and, for any framer used
    /// in a `while let Some(frame)` loop, greater than zero: a framer that
    /// reports zero consumed bytes would hand back the same frame forever.
    /// Framers in this crate uphold both invariants. The first one is checked
    /// with a `debug_assert!`.
    #[inline]
    #[must_use]
    pub const fn new(payload: &'a [u8], consumed: usize) -> Self {
        debug_assert!(
            consumed >= payload.len(),
            "Frame::consumed must cover the payload"
        );
        Self { payload, consumed }
    }

    /// Zero-copy borrow of the frame payload.
    #[inline]
    #[must_use]
    pub const fn payload(&self) -> &'a [u8] {
        self.payload
    }

    /// Number of bytes the framer consumed from the input to produce this frame.
    #[inline]
    #[must_use]
    pub const fn consumed(&self) -> usize {
        self.consumed
    }
}

/// Strategy that extracts and emits one frame at a time.
///
/// Implementations are stateless: each call to [`Framer::next_frame`] is
/// independent. To process a stream, the caller advances their own read
/// position by [`Frame::consumed`] after each successful call.
///
/// # Example
///
/// ```
/// use wire_codec::framing::{Framer, LengthPrefixed, LengthWidth, Endian};
///
/// let framer = LengthPrefixed::new(LengthWidth::U16, Endian::Big);
/// let input = &[0x00, 0x03, b'h', b'i', b'!'];
/// let frame = framer.next_frame(input).unwrap().unwrap();
/// assert_eq!(frame.payload(), b"hi!");
/// assert_eq!(frame.consumed(), 5);
/// ```
pub trait Framer {
    /// Try to extract a single frame from the start of `input`.
    ///
    /// Returns `Ok(Some(frame))` when a complete frame is available,
    /// `Ok(None)` when more bytes are needed, and `Err(_)` when the input
    /// violates the protocol.
    ///
    /// # Errors
    ///
    /// Implementation-defined; see each concrete framer for its error contract.
    fn next_frame<'a>(&self, input: &'a [u8]) -> Result<Option<Frame<'a>>>;

    /// Serialize `payload` as a single frame into `out`.
    ///
    /// The built-in framers are atomic: on error, `out` is left at its
    /// original position.
    ///
    /// # Errors
    ///
    /// Implementation-defined; see each concrete framer.
    fn write_frame(&self, payload: &[u8], out: &mut WriteBuf<'_>) -> Result<()>;

    /// Resumable variant of [`Framer::next_frame`] for streaming input.
    ///
    /// `scanned` is a progress hint owned by the caller. Start it at `0`.
    /// Between calls the caller may only append bytes to `input`; when a call
    /// returns `Ok(Some(frame))`, the framer resets `scanned` to `0` and the
    /// caller drops `frame.consumed()` bytes from the front before the next
    /// call. A framer that searches its input (such as [`Delimited`]) records
    /// how far it got on `Ok(None)` and continues from there next time, which
    /// turns a byte-at-a-time stream from quadratic into linear work.
    ///
    /// The default implementation ignores the hint and calls
    /// [`Framer::next_frame`], which is optimal for framers that read a
    /// fixed-position header. [`StreamDecoder`] manages the hint for you.
    ///
    /// # Errors
    ///
    /// Same contract as [`Framer::next_frame`].
    ///
    /// # Example
    ///
    /// ```
    /// use wire_codec::framing::{Delimited, Framer};
    ///
    /// let framer = Delimited::bounded(b"\n").unwrap();
    /// let mut scanned = 0;
    /// assert_eq!(framer.next_frame_from(b"hel", &mut scanned).unwrap(), None);
    /// assert_eq!(scanned, 3);
    /// let frame = framer.next_frame_from(b"hello\n", &mut scanned).unwrap().unwrap();
    /// assert_eq!(frame.payload(), b"hello");
    /// assert_eq!(scanned, 0);
    /// ```
    #[inline]
    fn next_frame_from<'a>(
        &self,
        input: &'a [u8],
        scanned: &mut usize,
    ) -> Result<Option<Frame<'a>>> {
        let result = self.next_frame(input);
        if let Ok(Some(_)) = result {
            *scanned = 0;
        }
        result
    }

    /// Upper bound on the number of bytes [`Framer::write_frame`] writes for
    /// a payload of `payload_len` bytes.
    ///
    /// Returns `None` when the framer cannot say, or when the bound would
    /// overflow `usize`. Buffer-growing adapters (such as the optional
    /// `bytes` codec) use it to reserve space in one step. The default
    /// implementation returns `None`.
    ///
    /// # Example
    ///
    /// ```
    /// use wire_codec::framing::{Delimited, Framer};
    ///
    /// let framer = Delimited::new(b"\r\n").unwrap();
    /// assert_eq!(framer.max_frame_len(10), Some(12));
    /// ```
    #[inline]
    fn max_frame_len(&self, payload_len: usize) -> Option<usize> {
        let _ = payload_len;
        None
    }
}

/// Stateful wrapper that drives a [`Framer`] over a growing input buffer.
///
/// It owns the [`Framer::next_frame_from`] progress hint, so a delimiter
/// search resumes where the previous call stopped. Use it whenever input
/// arrives in pieces (a socket read loop, a serial port, a message queue).
///
/// The input contract is the one described on [`Framer::next_frame_from`]:
/// between calls, only append to the buffer; after a frame is returned, drop
/// its `consumed()` bytes from the front.
///
/// # Example
///
/// ```
/// use wire_codec::framing::{Delimited, StreamDecoder};
///
/// let mut decoder = StreamDecoder::new(Delimited::bounded(b"\n").unwrap());
/// let mut buffer: Vec<u8> = Vec::new();
/// let mut lines: Vec<Vec<u8>> = Vec::new();
///
/// for chunk in [&b"PI"[..], b"NG\nQU", b"IT\n"] {
///     buffer.extend_from_slice(chunk);
///     let mut start = 0;
///     while let Some(frame) = decoder.decode(&buffer[start..]).unwrap() {
///         lines.push(frame.payload().to_vec());
///         start += frame.consumed();
///     }
///     buffer.drain(..start);
/// }
/// assert_eq!(lines, [b"PING".to_vec(), b"QUIT".to_vec()]);
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq, Hash)]
pub struct StreamDecoder<F> {
    framer: F,
    scanned: usize,
}

impl<F: Framer> StreamDecoder<F> {
    /// Wrap `framer` with fresh streaming state.
    #[inline]
    #[must_use]
    pub const fn new(framer: F) -> Self {
        Self { framer, scanned: 0 }
    }

    /// Try to extract the next frame from the start of `input`.
    ///
    /// # Errors
    ///
    /// Propagates the wrapped framer's errors. After an error the stream is
    /// usually unrecoverable; call [`StreamDecoder::reset`] before reusing
    /// the decoder on a new stream.
    #[inline]
    pub fn decode<'a>(&mut self, input: &'a [u8]) -> Result<Option<Frame<'a>>> {
        self.framer.next_frame_from(input, &mut self.scanned)
    }

    /// Forget any partial-scan progress, for example after discarding the
    /// buffered input or switching to a new connection.
    #[inline]
    pub fn reset(&mut self) {
        self.scanned = 0;
    }

    /// Borrow the wrapped framer, for example to call
    /// [`Framer::write_frame`] on the outbound path.
    #[inline]
    #[must_use]
    pub const fn framer(&self) -> &F {
        &self.framer
    }

    /// Consume the decoder and return the wrapped framer.
    #[inline]
    #[must_use]
    pub fn into_inner(self) -> F {
        self.framer
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Error;

    /// A framer that only implements the two required methods, so the
    /// trait's provided methods are exercised with their defaults.
    struct FixedFour;

    impl Framer for FixedFour {
        fn next_frame<'a>(&self, input: &'a [u8]) -> Result<Option<Frame<'a>>> {
            Ok(input.get(..4).map(|p| Frame::new(p, 4)))
        }
        fn write_frame(&self, payload: &[u8], out: &mut WriteBuf<'_>) -> Result<()> {
            if payload.len() != 4 {
                return Err(Error::InvalidEncoding);
            }
            out.write_bytes(payload)
        }
    }

    #[test]
    fn default_next_frame_from_resets_hint_on_frame() {
        let mut scanned = 7;
        assert_eq!(FixedFour.next_frame_from(b"ab", &mut scanned), Ok(None));
        assert_eq!(scanned, 7, "hint is left alone when no frame is produced");
        let frame = FixedFour
            .next_frame_from(b"abcdef", &mut scanned)
            .unwrap()
            .unwrap();
        assert_eq!(frame.payload(), b"abcd");
        assert_eq!(scanned, 0);
    }

    #[test]
    fn default_max_frame_len_is_unknown() {
        assert_eq!(FixedFour.max_frame_len(4), None);
        let mut out = [0u8; 4];
        let mut buf = WriteBuf::new(&mut out);
        FixedFour.write_frame(b"wxyz", &mut buf).unwrap();
        assert_eq!(
            FixedFour.write_frame(b"w", &mut buf),
            Err(Error::InvalidEncoding)
        );
    }

    #[test]
    fn stream_decoder_accessors_and_reset() {
        let mut decoder = StreamDecoder::new(Delimited::new(b"\n").unwrap());
        assert_eq!(decoder.decode(b"abc").unwrap(), None);
        assert_eq!(decoder.scanned, 3);
        decoder.reset();
        assert_eq!(decoder.scanned, 0);
        assert_eq!(decoder.framer().delimiter(), b"\n");
        let framer = decoder.into_inner();
        assert_eq!(framer.delimiter(), b"\n");
    }
}
