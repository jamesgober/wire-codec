//! Consistent Overhead Byte Stuffing (COBS).
//!
//! COBS rewrites a payload so that it contains no `0x00` bytes, at a cost of
//! at most one byte per 254 bytes of input (plus one). A zero byte can then
//! mark frame boundaries unambiguously, which makes COBS a common choice for
//! serial links, USB CDC, and radio packets where a receiver must be able to
//! resynchronise after noise. It is the framing used by `postcard`.
//!
//! This module only does the byte stuffing. Combine it with
//! [`Delimited`][`crate::framing::Delimited`] on a `[0x00]` delimiter to cut
//! the stream into frames:
//!
//! ```
//! use wire_codec::framing::{Delimited, Framer};
//! use wire_codec::{cobs, WriteBuf};
//!
//! // Sender: stuff the payload, then terminate it with 0x00.
//! let payload = [0x11, 0x00, 0x22, 0x00];
//! let mut wire = [0u8; 16];
//! let mut out = WriteBuf::new(&mut wire);
//! cobs::encode(&payload, &mut out).unwrap();
//! out.write_u8(0x00).unwrap();
//! let n = out.position();
//! assert_eq!(&wire[..n], &[0x02, 0x11, 0x02, 0x22, 0x01, 0x00]);
//!
//! // Receiver: split on 0x00, then unstuff in place.
//! let framer = Delimited::bounded(&[0x00]).unwrap();
//! let frame = framer.next_frame(&wire[..n]).unwrap().unwrap();
//! let mut block = [0u8; 16];
//! let block = &mut block[..frame.payload().len()];
//! block.copy_from_slice(frame.payload());
//! let len = cobs::decode_in_place(block).unwrap();
//! assert_eq!(&block[..len], &payload);
//! ```
//!
//! All functions are allocation-free and atomic: on error nothing is written
//! to the output cursor.

use crate::buf::WriteBuf;
use crate::error::{Error, Result};

/// Largest number of data bytes in one COBS block.
const MAX_RUN: usize = 254;

/// Upper bound on the encoded size of a `len`-byte payload, excluding the
/// trailing `0x00` frame delimiter: `len + ceil(len / 254)`, and at least 1.
///
/// Saturates at `usize::MAX`.
///
/// # Example
///
/// ```
/// use wire_codec::cobs;
/// assert_eq!(cobs::max_encoded_len(0), 1);
/// assert_eq!(cobs::max_encoded_len(254), 255);
/// assert_eq!(cobs::max_encoded_len(255), 257);
/// ```
#[inline]
#[must_use]
pub const fn max_encoded_len(len: usize) -> usize {
    let overhead = if len == 0 { 1 } else { len.div_ceil(MAX_RUN) };
    len.saturating_add(overhead)
}

/// COBS-encode `src` into `out`. The output contains no `0x00` bytes and
/// does not include a trailing delimiter.
///
/// A full 254-byte block at the end of the input is not followed by an
/// empty block, so the output is never longer than
/// [`max_encoded_len`]`(src.len())`. This matches the reference encoder of
/// Cheshire and Baker and the `cobs` and `postcard` crates.
///
/// # Errors
///
/// Returns [`Error::BufferFull`] if `out` has insufficient room. Nothing is
/// written on error.
///
/// # Example
///
/// ```
/// use wire_codec::{cobs, WriteBuf};
///
/// let mut out = [0u8; 8];
/// let mut buf = WriteBuf::new(&mut out);
/// cobs::encode(&[0x00], &mut buf).unwrap();
/// assert_eq!(buf.written(), &[0x01, 0x01]);
/// ```
pub fn encode(src: &[u8], out: &mut WriteBuf<'_>) -> Result<()> {
    let dst = out.spare_mut();
    // Index of the code byte of the block currently being filled.
    let mut code_at = 0;
    let mut write = 1;
    let mut code: u8 = 1;
    if dst.is_empty() {
        return Err(Error::BufferFull);
    }
    for (i, &byte) in src.iter().enumerate() {
        if byte != 0 {
            *dst.get_mut(write).ok_or(Error::BufferFull)? = byte;
            write += 1;
            code += 1;
        }
        let more_input = i + 1 < src.len();
        if byte == 0 || (code == 0xFF && more_input) {
            // Close the block and reserve a code byte for the next one.
            dst[code_at] = code;
            code_at = write;
            if code_at >= dst.len() {
                return Err(Error::BufferFull);
            }
            write += 1;
            code = 1;
        }
    }
    dst[code_at] = code;
    out.commit(write);
    Ok(())
}

/// Decode one COBS block (without its `0x00` delimiter) from `src` into
/// `out`.
///
/// # Errors
///
/// - [`Error::InvalidEncoding`] if `src` is empty, contains a `0x00` byte, or
///   a code byte points past the end of the block.
/// - [`Error::BufferFull`] if `out` has insufficient room. The decoded size
///   is always smaller than `src.len()`.
///
/// Nothing is written on error.
///
/// # Example
///
/// ```
/// use wire_codec::{cobs, WriteBuf};
///
/// let mut out = [0u8; 8];
/// let mut buf = WriteBuf::new(&mut out);
/// cobs::decode(&[0x03, 0x11, 0x22, 0x02, 0x33], &mut buf).unwrap();
/// assert_eq!(buf.written(), &[0x11, 0x22, 0x00, 0x33]);
/// ```
pub fn decode(src: &[u8], out: &mut WriteBuf<'_>) -> Result<()> {
    let dst = out.spare_mut();
    let mut write = 0;
    for_each_run(src, |run, zero_follows| {
        let end = write + run.len();
        dst.get_mut(write..end)
            .ok_or(Error::BufferFull)?
            .copy_from_slice(run);
        write = end;
        if zero_follows {
            *dst.get_mut(write).ok_or(Error::BufferFull)? = 0;
            write += 1;
        }
        Ok(())
    })?;
    out.commit(write);
    Ok(())
}

/// Decode one COBS block in place and return the decoded length.
///
/// The decoded bytes occupy `buf[..len]`; the rest of `buf` is left in an
/// unspecified state. Decoding never needs more room than the input, so this
/// works on the receive buffer itself.
///
/// # Errors
///
/// Returns [`Error::InvalidEncoding`] under the same conditions as
/// [`decode`]. `buf` may be partially modified on error.
///
/// # Example
///
/// ```
/// use wire_codec::cobs;
///
/// let mut block = [0x02, 0x11, 0x01, 0x01];
/// let len = cobs::decode_in_place(&mut block).unwrap();
/// assert_eq!(&block[..len], &[0x11, 0x00, 0x00]);
/// ```
pub fn decode_in_place(buf: &mut [u8]) -> Result<usize> {
    validate(buf)?;
    let mut read = 0;
    let mut write = 0;
    while read < buf.len() {
        let code = usize::from(buf[read]);
        let run_start = read + 1;
        let run_end = read + code;
        buf.copy_within(run_start..run_end, write);
        write += code - 1;
        read = run_end;
        if code != 0xFF && read < buf.len() {
            buf[write] = 0;
            write += 1;
        }
    }
    Ok(write)
}

/// Check that `src` is a structurally valid COBS block.
fn validate(src: &[u8]) -> Result<()> {
    for_each_run(src, |_, _| Ok(()))
}

/// Walk the runs of a COBS block, calling `emit(run, zero_follows)` for each.
fn for_each_run<'a>(
    src: &'a [u8],
    mut emit: impl FnMut(&'a [u8], bool) -> Result<()>,
) -> Result<()> {
    if src.is_empty() {
        return Err(Error::InvalidEncoding);
    }
    let mut read = 0;
    while read < src.len() {
        let code = usize::from(src[read]);
        if code == 0 {
            return Err(Error::InvalidEncoding);
        }
        let run = src
            .get(read + 1..read + code)
            .ok_or(Error::InvalidEncoding)?;
        if run.contains(&0) {
            return Err(Error::InvalidEncoding);
        }
        read += code;
        emit(run, code != 0xFF && read < src.len())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode_vec(src: &[u8]) -> Vec<u8> {
        let mut out = vec![0u8; max_encoded_len(src.len())];
        let mut buf = WriteBuf::new(&mut out);
        encode(src, &mut buf).unwrap();
        let n = buf.position();
        out.truncate(n);
        out
    }

    fn decode_vec(src: &[u8]) -> Result<Vec<u8>> {
        let mut out = vec![0u8; src.len()];
        let mut buf = WriteBuf::new(&mut out);
        decode(src, &mut buf)?;
        let n = buf.position();
        out.truncate(n);
        Ok(out)
    }

    #[test]
    fn wikipedia_vectors() {
        let cases: [(&[u8], &[u8]); 6] = [
            (&[], &[0x01]),
            (&[0x00], &[0x01, 0x01]),
            (&[0x00, 0x00], &[0x01, 0x01, 0x01]),
            (&[0x00, 0x11, 0x00], &[0x01, 0x02, 0x11, 0x01]),
            (&[0x11, 0x22, 0x00, 0x33], &[0x03, 0x11, 0x22, 0x02, 0x33]),
            (&[0x11, 0x22, 0x33, 0x44], &[0x05, 0x11, 0x22, 0x33, 0x44]),
        ];
        for (raw, encoded) in cases {
            assert_eq!(encode_vec(raw), encoded, "encode {raw:?}");
            assert_eq!(decode_vec(encoded).unwrap(), raw, "decode {encoded:?}");
            let mut block = encoded.to_vec();
            let len = decode_in_place(&mut block).unwrap();
            assert_eq!(&block[..len], raw);
        }
    }

    #[test]
    fn long_runs() {
        // 254 non-zero bytes: one full block, no trailing empty block.
        let raw: Vec<u8> = (1..=254).map(|i| i as u8).collect();
        let encoded = encode_vec(&raw);
        assert_eq!(encoded.len(), 255);
        assert_eq!(encoded[0], 0xFF);
        assert_eq!(decode_vec(&encoded).unwrap(), raw);

        // 255 non-zero bytes: a full block, then a one-byte block.
        let raw: Vec<u8> = (0..255).map(|i| (i % 255) as u8 + 1).collect();
        let encoded = encode_vec(&raw);
        assert_eq!(encoded.len(), max_encoded_len(255));
        assert_eq!(&encoded[255..], &[0x02, raw[254]]);
        assert_eq!(decode_vec(&encoded).unwrap(), raw);

        // A full block followed by a zero.
        let mut raw: Vec<u8> = vec![0x7F; 254];
        raw.push(0);
        let encoded = encode_vec(&raw);
        assert_eq!(decode_vec(&encoded).unwrap(), raw);
    }

    #[test]
    fn round_trip_mixed_lengths() {
        for len in [0usize, 1, 253, 254, 255, 508, 509, 1000] {
            let raw: Vec<u8> = (0..len).map(|i| (i * 7 % 5) as u8).collect();
            let encoded = encode_vec(&raw);
            assert!(encoded.len() <= max_encoded_len(len));
            assert!(!encoded.contains(&0));
            assert_eq!(decode_vec(&encoded).unwrap(), raw);
        }
    }

    #[test]
    fn malformed_blocks() {
        let cases: [&[u8]; 5] = [
            &[],
            &[0x00],
            &[0x03, 0x11],
            &[0x02, 0x00],
            &[0x02, 0x11, 0x00, 0x01],
        ];
        for bad in cases {
            assert_eq!(decode_vec(bad), Err(Error::InvalidEncoding), "{bad:?}");
            let mut block = bad.to_vec();
            assert_eq!(decode_in_place(&mut block), Err(Error::InvalidEncoding));
        }
    }

    #[test]
    fn encode_is_atomic_on_buffer_full() {
        for (src, room) in [
            (&[0x11u8, 0x22][..], 2usize),
            (&[0x00, 0x00], 2),
            (&[0x11], 0),
        ] {
            let mut out = vec![0xEEu8; room];
            let mut buf = WriteBuf::new(&mut out);
            assert_eq!(encode(src, &mut buf), Err(Error::BufferFull), "{src:?}");
            assert_eq!(buf.position(), 0);
        }
    }

    #[test]
    fn decode_is_atomic_on_buffer_full() {
        let mut out = [0u8; 2];
        let mut buf = WriteBuf::new(&mut out);
        assert_eq!(decode(&[0x04, 1, 2, 3], &mut buf), Err(Error::BufferFull));
        assert_eq!(
            decode(&[0x02, 1, 0x02, 2], &mut buf),
            Err(Error::BufferFull)
        );
        assert_eq!(buf.position(), 0);
    }

    #[test]
    fn max_encoded_len_saturates() {
        assert_eq!(max_encoded_len(usize::MAX), usize::MAX);
    }
}
