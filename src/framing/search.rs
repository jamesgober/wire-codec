//! Substring search used by the delimited framer.
//!
//! The default build uses a portable SWAR (SIMD-within-a-register) scan, 16
//! bytes per iteration, in safe code with no target features. A single-byte
//! delimiter is one SWAR scan. A longer delimiter is found by scanning for its
//! **last** byte and checking the bytes before it at each hit; if hits keep
//! failing (input built to match the delimiter's last byte everywhere), the
//! search switches to Horspool's skip table for the rest of the input, so
//! hostile input costs a few cheap steps per byte instead of a full compare
//! at every position. With the optional `memchr` feature the search is
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
    match needle {
        [] => None,
        [byte] => find_byte(*byte, haystack),
        _ => find_multi(haystack, needle),
    }
}

/// Failed candidates the last-byte scan tolerates before it switches to
/// Horspool: honest input rarely has more than a few, hostile input has one
/// at every position.
#[cfg(any(not(feature = "memchr"), test))]
const MISSES_BEFORE_SKIP_TABLE: u32 = 8;

/// Multi-byte search: SWAR scan for the needle's last byte, verify the bytes
/// before it, and fall back to [`horspool`] once candidates keep failing.
/// `needle.len() >= 2`.
#[cfg(any(not(feature = "memchr"), test))]
fn find_multi(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    let (&last, prefix) = needle.split_last()?;
    let m = needle.len();
    if haystack.len() < m {
        return None;
    }
    // `end` is where the next window's last byte may sit.
    let mut end = m - 1;
    let mut misses = 0;
    while end < haystack.len() {
        let hit = end + find_byte(last, &haystack[end..])?;
        let start = hit + 1 - m;
        count_verify();
        if haystack[start..hit] == *prefix {
            return Some(start);
        }
        misses += 1;
        if misses >= MISSES_BEFORE_SKIP_TABLE {
            return horspool(haystack, needle, start + 1);
        }
        end = hit + 1;
    }
    None
}

/// Boyer-Moore-Horspool from window start `from`: compare the window's last
/// byte, then the rest, and shift by the skip table entry of the byte under
/// the window's end. `needle.len() >= 2`.
#[cfg(any(not(feature = "memchr"), test))]
fn horspool(haystack: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    let m = needle.len();
    let mut skip = [m; 256];
    for (i, &b) in needle[..m - 1].iter().enumerate() {
        skip[usize::from(b)] = m - 1 - i;
    }
    let last = needle[m - 1];
    let mut start = from;
    while start + m <= haystack.len() {
        let tail = haystack[start + m - 1];
        if tail == last {
            count_verify();
            if haystack[start..start + m - 1] == needle[..m - 1] {
                return Some(start);
            }
        }
        start += skip[usize::from(tail)];
    }
    None
}

#[cfg(test)]
thread_local! {
    /// Full comparisons made by the multi-byte search, for the cost tests.
    static VERIFIES: core::cell::Cell<u64> = const { core::cell::Cell::new(0) };
}

#[cfg(test)]
fn count_verify() {
    VERIFIES.with(|v| v.set(v.get() + 1));
}

#[cfg(all(not(test), not(feature = "memchr")))]
#[inline(always)]
fn count_verify() {}

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

    #[test]
    fn multi_byte_search_agrees_with_naive_on_dense_input() {
        // Inputs where the delimiter's bytes are everywhere, so the search
        // goes through the skip-table path as well as the scan.
        const CRLF2: &[u8] = &[13, 10, 13, 10];
        let needles: [&[u8]; 6] = [b"aaab", CRLF2, b"abab", b"ba", b"aab", b"xyzx"];
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        for needle in needles {
            for len in [0usize, 3, 17, 64, 300] {
                for _ in 0..40 {
                    let hay: Vec<u8> = (0..len)
                        .map(|_| {
                            seed ^= seed << 13;
                            seed ^= seed >> 7;
                            seed ^= seed << 17;
                            needle[(seed % needle.len() as u64) as usize]
                        })
                        .collect();
                    for start in [0, 1, len / 2] {
                        assert_eq!(
                            find_multi_from(&hay, needle, start),
                            naive(&hay, needle, start),
                            "{needle:?} {hay:?} {start}"
                        );
                    }
                }
            }
        }
    }

    fn find_multi_from(haystack: &[u8], needle: &[u8], start: usize) -> Option<usize> {
        haystack
            .get(start..)
            .and_then(|tail| find_multi(tail, needle))
            .map(|pos| pos + start)
    }

    /// Hostile input used to cost one full compare per byte (the first-byte
    /// scan stopped at every position). It now costs a bounded number of
    /// failed candidates, then at most one compare per skip-table step.
    #[test]
    fn hostile_multi_byte_search_stays_cheap() {
        let n = 64 * 1024;
        // CR LF CR LF over a run of CRs and over a run of LFs.
        const CRLF2: &[u8] = &[13, 10, 13, 10];
        let cases: [(&[u8], u8); 4] = [(b"aaab", b'a'), (CRLF2, 13), (CRLF2, 10), (b"abab", b'b')];
        for (needle, fill) in cases {
            let hay = vec![fill; n];
            VERIFIES.with(|v| v.set(0));
            assert_eq!(find_multi(&hay, needle), None);
            let verifies = VERIFIES.with(core::cell::Cell::get);
            assert!(
                verifies <= (n / 2) as u64 + u64::from(MISSES_BEFORE_SKIP_TABLE),
                "{needle:?} over {fill:?}: {verifies} compares for {n} bytes"
            );
        }
        // Where the delimiter's last byte never appears, no compare at all.
        let hay = vec![b'a'; n];
        VERIFIES.with(|v| v.set(0));
        assert_eq!(find_multi(&hay, b"aaab"), None);
        assert_eq!(VERIFIES.with(core::cell::Cell::get), 0);
    }
}
