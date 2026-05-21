//! Frame extraction strategies.
//!
//! A [`Framer`] is a stateless protocol that locates and emits one frame at a
//! time from a borrowed byte slice. Frames are described by [`Frame`], which
//! carries a zero-copy reference to the payload alongside the count of bytes
//! the framer consumed from the input.
//!
//! Two concrete framers ship with the crate:
//!
//! - [`length::LengthPrefixed`] for fixed-width length-prefixed protocols.
//! - [`delimiter::Delimited`] for byte-delimited protocols such as newline-
//!   terminated text.
//!
//! Custom framers are implemented by writing a `Framer` impl on a user-defined
//! type.

pub mod delimiter;
pub mod length;

pub use delimiter::Delimited;
pub use length::{Endian, LengthPrefixed, LengthWidth};

use crate::buf::WriteBuf;
use crate::error::Result;

/// A frame extracted from an input byte slice.
///
/// The `payload` borrow lives as long as the input slice, so framers never
/// copy data. `consumed` tells the caller how many bytes from the input the
/// frame occupied in total, including any header or delimiter bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Frame<'a> {
    payload: &'a [u8],
    consumed: usize,
}

impl<'a> Frame<'a> {
    /// Construct a frame from its payload and total consumed-byte count.
    ///
    /// `consumed` must be at least `payload.len()`; framers in this crate
    /// uphold that invariant.
    #[inline]
    pub const fn new(payload: &'a [u8], consumed: usize) -> Self {
        Self { payload, consumed }
    }

    /// Zero-copy borrow of the frame payload.
    #[inline]
    pub const fn payload(&self) -> &'a [u8] {
        self.payload
    }

    /// Number of bytes the framer consumed from the input to produce this frame.
    #[inline]
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
    /// # Errors
    ///
    /// Implementation-defined; see each concrete framer.
    fn write_frame(&self, payload: &[u8], out: &mut WriteBuf<'_>) -> Result<()>;
}
