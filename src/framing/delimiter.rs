//! Byte-delimited framing.
//!
//! Frames are separated by a configured delimiter byte sequence (typically
//! `\n` for newline-terminated text, or `\r\n` for HTTP-style protocols). The
//! delimiter is stripped from the emitted payload but counted toward
//! [`Frame::consumed`].

use crate::buf::WriteBuf;
use crate::error::{Error, Result};
use crate::framing::{Frame, Framer};

/// Byte-delimited framer.
///
/// # Example
///
/// ```
/// use wire_codec::WriteBuf;
/// use wire_codec::framing::{Delimited, Framer};
///
/// let framer = Delimited::new(b"\n").unwrap();
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
    /// Build a framer using `delimiter` to separate frames.
    ///
    /// # Errors
    ///
    /// Returns [`Error::EmptyDelimiter`] if `delimiter` is empty. An empty
    /// delimiter cannot uniquely separate frames.
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

    /// Set an upper bound on payload size. Frames whose payload would exceed
    /// this size cause [`Error::FrameTooLarge`].
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

    fn find_delimiter(&self, haystack: &[u8]) -> Option<usize> {
        let needle = self.delimiter;
        if haystack.len() < needle.len() {
            return None;
        }
        let last = haystack.len() - needle.len();
        let mut i = 0;
        while i <= last {
            if &haystack[i..i + needle.len()] == needle {
                return Some(i);
            }
            i += 1;
        }
        None
    }
}

impl<'d> Framer for Delimited<'d> {
    fn next_frame<'a>(&self, input: &'a [u8]) -> Result<Option<Frame<'a>>> {
        // Cap the search window at max_payload + delimiter length. Saturating
        // because the default max_payload is usize::MAX.
        let window_cap = self.max_payload.saturating_add(self.delimiter.len());
        let scan_window = if input.len() > window_cap {
            &input[..window_cap]
        } else {
            input
        };
        match self.find_delimiter(scan_window) {
            Some(pos) => {
                if pos > self.max_payload {
                    return Err(Error::FrameTooLarge {
                        len: pos,
                        limit: self.max_payload,
                    });
                }
                let consumed = pos + self.delimiter.len();
                Ok(Some(Frame::new(&input[..pos], consumed)))
            }
            None => {
                // No delimiter inside the window, and the input has already
                // exceeded the limit: the protocol is broken.
                if input.len() > window_cap {
                    return Err(Error::FrameTooLarge {
                        len: input.len(),
                        limit: self.max_payload,
                    });
                }
                Ok(None)
            }
        }
    }

    fn write_frame(&self, payload: &[u8], out: &mut WriteBuf<'_>) -> Result<()> {
        if payload.len() > self.max_payload {
            return Err(Error::FrameTooLarge {
                len: payload.len(),
                limit: self.max_payload,
            });
        }
        out.write_bytes(payload)?;
        out.write_bytes(self.delimiter)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(matches!(result, Err(Error::FrameTooLarge { limit: 3, .. })));
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
        let result = Delimited::new(b"");
        assert!(matches!(result, Err(Error::EmptyDelimiter)));
    }
}
