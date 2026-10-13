//! Substring search used by the delimited framer.
//!
//! The default build uses a portable SWAR (SIMD-within-a-register) scan for
//! the delimiter's first byte, then verifies the remaining bytes at each
//! candidate. It is safe code, needs no target features, and processes 16
//! bytes per iteration. With the optional `memchr` feature the search is
//! delegated to the `memchr` crate, which adds runtime-dispatched SSE2/AVX2
//! and NEON kernels and a linear-time two-way search for multi-byte
//! delimiters.

/// Find the first occurrence of `needle` in `haystack` that starts at or
/// after `start`. `needle` must be non-empty.
#[inline]
pub(crate) fn find(haystack: &[u8], needle: &[u8], start: usize) -> Option<usize> {
    let tail = haystack.get(start..)?;
    find_in(tail, needle).map(|pos| pos + start)
}

#[cfg(feature = "memchr")]
#[inline]
fn find_in(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    match needle {
        [byte] => memchr::memchr(*byte, haystack),
        _ => memchr::memmem::find(haystack, needle),
    }
}

#[cfg(not(feature = "memchr"))]
#[inline]
fn find_in(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    let (&first, rest) = needle.split_first()?;
    // Last offset at which a full needle still fits.
    let last_start = haystack.len().checked_sub(needle.len())?;
    let mut from = 0;
    while from <= last_start {
        let candidate = from + find_byte(first, &haystack[from..=last_start])?;
        let after = candidate + 1;
        if haystack[after..after + rest.len()] == *rest {
            return Some(candidate);
        }
        from = after;
    }
    None
}

/// Position of the first `needle` byte in `haystack`.
///
/// Classic "has zero byte" bit trick: XOR every byte with `needle` so that
/// matches become `0x00`, then `(x - 0x01..01) & !x & 0x80..80` flags them.
/// The lowest flagged byte is always a true match (borrows only propagate
/// upward), and loading little-endian makes "lowest" mean "first in memory".
#[cfg(any(not(feature = "memchr"), test))]
#[inline]
pub(crate) fn find_byte(needle: u8, haystack: &[u8]) -> Option<usize> {
    const LO: u64 = 0x0101_0101_0101_0101;
    const HI: u64 = 0x8080_8080_8080_8080;
    let pattern = LO * u64::from(needle);

    let flags = |chunk: &[u8]| -> u64 {
        let mut word = [0u8; 8];
        word.copy_from_slice(chunk);
        let x = u64::from_le_bytes(word) ^ pattern;
        x.wrapping_sub(LO) & !x & HI
    };

    let mut offset = 0;
    let mut chunks = haystack.chunks_exact(16);
    for chunk in &mut chunks {
        let (lo, hi) = chunk.split_at(8);
        let (a, b) = (flags(lo), flags(hi));
        if a | b != 0 {
            let hit = if a != 0 {
                a.trailing_zeros()
            } else {
                64 + b.trailing_zeros()
            };
            return Some(offset + (hit / 8) as usize);
        }
        offset += 16;
    }
    chunks
        .remainder()
        .iter()
        .position(|&b| b == needle)
        .map(|pos| offset + pos)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn naive(haystack: &[u8], needle: &[u8], start: usize) -> Option<usize> {
        if needle.len() > haystack.len() {
            return None;
        }
        (start..=haystack.len() - needle.len()).find(|&i| &haystack[i..i + needle.len()] == needle)
    }

    #[test]
    fn find_byte_matches_position_for_every_offset() {
        for len in 0..80 {
            let mut hay = vec![0xAAu8; len];
            assert_eq!(find_byte(b'\n', &hay), None);
            for at in 0..len {
                hay[at] = b'\n';
                assert_eq!(find_byte(b'\n', &hay), Some(at), "len={len} at={at}");
                // A second, later match must not shadow the first.
                if at + 1 < len {
                    hay[len - 1] = b'\n';
                    assert_eq!(find_byte(b'\n', &hay), Some(at));
                    hay[len - 1] = 0xAA;
                }
                hay[at] = 0xAA;
            }
        }
    }

    #[test]
    fn find_byte_handles_borrow_propagation() {
        // 0x01 right above a match is the classic false-positive pattern for
        // the zero-byte trick; the lowest flag must still be the true match.
        let hay = [0x00u8, 0x01, 0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06];
        assert_eq!(find_byte(0x01, &hay), Some(1));
        assert_eq!(find_byte(0x00, &hay), Some(0));
        assert_eq!(find_byte(0x06, &hay), Some(8));
        assert_eq!(find_byte(0xFF, &[0xFE; 33]), None);
    }

    #[test]
    fn find_agrees_with_naive_search() {
        let hay = b"a\r\r\nbc\r\n\r\r\r\nxyz\rabcabcd";
        for needle in [&b"\r\n"[..], b"\n", b"\r", b"abcd", b"zz", b"\r\r\n", b"a"] {
            for start in 0..=hay.len() + 1 {
                assert_eq!(
                    find(hay, needle, start),
                    naive(hay, needle, start),
                    "{needle:?} {start}"
                );
            }
        }
        assert_eq!(find(b"ab", b"abc", 0), None);
        assert_eq!(find(b"", b"a", 0), None);
    }
}
