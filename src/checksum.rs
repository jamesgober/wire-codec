//! Frame checksums.
//!
//! The [`Checksum`] trait describes an integrity check that produces a fixed
//! number of bytes. Three common CRCs are built in, implemented locally with
//! `const`-generated lookup tables (slice-by-8 for the CRC-32 variants) so
//! the crate stays dependency-free:
//!
//! | Type | Algorithm (catalogue name) | Width | Typical use |
//! | ---- | -------------------------- | ----- | ----------- |
//! | [`Crc16CcittFalse`] | CRC-16/IBM-3740 (a.k.a. CRC-16/CCITT-FALSE) | 2 bytes | Serial links, Bluetooth, many embedded protocols |
//! | [`Crc32`] | CRC-32/ISO-HDLC (IEEE 802.3, zlib, PNG) | 4 bytes | Ethernet, gzip, general purpose |
//! | [`Crc32c`] | CRC-32/ISCSI (Castagnoli) | 4 bytes | iSCSI, SCTP, ext4, Kafka record batches |
//!
//! Pair a checksum with any framer through
//! [`Checksummed`][`crate::framing::Checksummed`]. Implement [`Checksum`]
//! yourself to plug in a different algorithm (for example a hardware CRC
//! unit on a microcontroller).
//!
//! A CRC detects accidental corruption. It is not a MAC: anyone can compute
//! a valid CRC for forged data. Use an authenticated cipher or a keyed MAC
//! when the peer is adversarial.

/// An integrity check over a byte slice that yields [`Checksum::LEN`] bytes.
///
/// # Example
///
/// A one-byte XOR checksum (weak, but a short example):
///
/// ```
/// use wire_codec::checksum::Checksum;
///
/// struct Xor8;
///
/// impl Checksum for Xor8 {
///     const LEN: usize = 1;
///     fn compute(&self, data: &[u8]) -> u64 {
///         u64::from(data.iter().fold(0u8, |acc, b| acc ^ b))
///     }
/// }
///
/// assert_eq!(Xor8.compute(&[0x0F, 0xF0]), 0xFF);
/// ```
pub trait Checksum {
    /// Number of bytes the checksum occupies on the wire. Must be in `1..=8`.
    const LEN: usize;

    /// Compute the checksum of `data`.
    ///
    /// Only the low `LEN * 8` bits of the result are transmitted.
    fn compute(&self, data: &[u8]) -> u64;
}

/// CRC-16/IBM-3740, widely known as CRC-16/CCITT-FALSE.
///
/// Polynomial `0x1021`, initial value `0xFFFF`, no reflection, no final XOR.
/// Check value for `b"123456789"`: `0x29B1`.
///
/// # Example
///
/// ```
/// use wire_codec::checksum::{Checksum, Crc16CcittFalse};
///
/// assert_eq!(Crc16CcittFalse.compute(b"123456789"), 0x29B1);
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Crc16CcittFalse;

impl Checksum for Crc16CcittFalse {
    const LEN: usize = 2;

    #[inline]
    fn compute(&self, data: &[u8]) -> u64 {
        u64::from(crc16_ccitt_false(data))
    }
}

/// CRC-32/ISO-HDLC, the CRC-32 of IEEE 802.3, zlib, gzip, and PNG.
///
/// Reflected polynomial `0xEDB88320`, initial value and final XOR
/// `0xFFFFFFFF`. Check value for `b"123456789"`: `0xCBF43926`.
///
/// # Example
///
/// ```
/// use wire_codec::checksum::{Checksum, Crc32};
///
/// assert_eq!(Crc32.compute(b"123456789"), 0xCBF4_3926);
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Crc32;

impl Checksum for Crc32 {
    const LEN: usize = 4;

    #[inline]
    fn compute(&self, data: &[u8]) -> u64 {
        u64::from(crc32(data))
    }
}

/// CRC-32/ISCSI (Castagnoli), used by iSCSI, SCTP, ext4, and Kafka.
///
/// Reflected polynomial `0x82F63B78`, initial value and final XOR
/// `0xFFFFFFFF`. Check value for `b"123456789"`: `0xE3069283`.
///
/// # Example
///
/// ```
/// use wire_codec::checksum::{Checksum, Crc32c};
///
/// assert_eq!(Crc32c.compute(b"123456789"), 0xE306_9283);
/// ```
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Crc32c;

impl Checksum for Crc32c {
    const LEN: usize = 4;

    #[inline]
    fn compute(&self, data: &[u8]) -> u64 {
        u64::from(crc32c(data))
    }
}

/// Compute CRC-16/IBM-3740 (CRC-16/CCITT-FALSE) of `data`.
///
/// # Example
///
/// ```
/// use wire_codec::checksum;
/// assert_eq!(checksum::crc16_ccitt_false(b"123456789"), 0x29B1);
/// assert_eq!(checksum::crc16_ccitt_false(b""), 0xFFFF);
/// ```
#[inline]
#[must_use]
pub fn crc16_ccitt_false(data: &[u8]) -> u16 {
    data.iter().fold(0xFFFF_u16, |crc, &byte| {
        let index = usize::from((crc >> 8) as u8 ^ byte);
        (crc << 8) ^ CRC16_CCITT_TABLE[index]
    })
}

/// Compute CRC-32/ISO-HDLC (IEEE) of `data`.
///
/// # Example
///
/// ```
/// use wire_codec::checksum;
/// assert_eq!(checksum::crc32(b"123456789"), 0xCBF4_3926);
/// assert_eq!(checksum::crc32(b""), 0);
/// ```
#[inline]
#[must_use]
pub fn crc32(data: &[u8]) -> u32 {
    reflected_crc32(&CRC32_TABLE, data)
}

/// Compute CRC-32/ISCSI (Castagnoli) of `data`.
///
/// # Example
///
/// ```
/// use wire_codec::checksum;
/// assert_eq!(checksum::crc32c(b"123456789"), 0xE306_9283);
/// ```
#[inline]
#[must_use]
pub fn crc32c(data: &[u8]) -> u32 {
    reflected_crc32(&CRC32C_TABLE, data)
}

#[inline]
fn reflected_crc32(tables: &[[u32; 256]; 8], data: &[u8]) -> u32 {
    // Slice-by-8: fold eight input bytes per step through eight derived
    // tables, then finish the tail one byte at a time.
    let mut crc = !0u32;
    let mut chunks = data.chunks_exact(8);
    for chunk in &mut chunks {
        let mut word = [0u8; 8];
        word.copy_from_slice(chunk);
        let lo = u32::from_le_bytes([word[0], word[1], word[2], word[3]]) ^ crc;
        let hi = u32::from_le_bytes([word[4], word[5], word[6], word[7]]);
        crc = tables[7][usize::from(lo as u8)]
            ^ tables[6][usize::from((lo >> 8) as u8)]
            ^ tables[5][usize::from((lo >> 16) as u8)]
            ^ tables[4][usize::from((lo >> 24) as u8)]
            ^ tables[3][usize::from(hi as u8)]
            ^ tables[2][usize::from((hi >> 8) as u8)]
            ^ tables[1][usize::from((hi >> 16) as u8)]
            ^ tables[0][usize::from((hi >> 24) as u8)];
    }
    for &byte in chunks.remainder() {
        crc = (crc >> 8) ^ tables[0][usize::from(crc as u8 ^ byte)];
    }
    !crc
}

static CRC16_CCITT_TABLE: [u16; 256] = crc16_table(0x1021);
static CRC32_TABLE: [[u32; 256]; 8] = slice_by_8(reflected_crc32_table(0xEDB8_8320));
static CRC32C_TABLE: [[u32; 256]; 8] = slice_by_8(reflected_crc32_table(0x82F6_3B78));

/// Byte-wise lookup table for an MSB-first (non-reflected) CRC-16.
const fn crc16_table(poly: u16) -> [u16; 256] {
    let mut table = [0u16; 256];
    let mut i = 0;
    while i < 256 {
        let mut crc = (i as u16) << 8;
        let mut bit = 0;
        while bit < 8 {
            crc = if crc & 0x8000 != 0 {
                (crc << 1) ^ poly
            } else {
                crc << 1
            };
            bit += 1;
        }
        table[i] = crc;
        i += 1;
    }
    table
}

/// Derive the eight slice-by-8 tables from the byte-wise table:
/// `tables[k][i]` is the CRC contribution of byte `i` followed by `k` zeros.
const fn slice_by_8(base: [u32; 256]) -> [[u32; 256]; 8] {
    let mut tables = [[0u32; 256]; 8];
    tables[0] = base;
    let mut k = 1;
    while k < 8 {
        let mut i = 0;
        while i < 256 {
            let prev = tables[k - 1][i];
            tables[k][i] = (prev >> 8) ^ base[(prev & 0xFF) as usize];
            i += 1;
        }
        k += 1;
    }
    tables
}

/// Byte-wise lookup table for an LSB-first (reflected) CRC-32.
const fn reflected_crc32_table(poly: u32) -> [u32; 256] {
    let mut table = [0u32; 256];
    let mut i = 0;
    while i < 256 {
        let mut crc = i as u32;
        let mut bit = 0;
        while bit < 8 {
            crc = if crc & 1 != 0 {
                (crc >> 1) ^ poly
            } else {
                crc >> 1
            };
            bit += 1;
        }
        table[i] = crc;
        i += 1;
    }
    table
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Bit-at-a-time reference implementations, straight from the catalogue
    /// parameters, to cross-check the table-driven versions.
    fn crc16_reference(data: &[u8]) -> u16 {
        let mut crc = 0xFFFFu16;
        for &byte in data {
            crc ^= u16::from(byte) << 8;
            for _ in 0..8 {
                crc = if crc & 0x8000 != 0 {
                    (crc << 1) ^ 0x1021
                } else {
                    crc << 1
                };
            }
        }
        crc
    }

    fn crc32_reference(poly: u32, data: &[u8]) -> u32 {
        let mut crc = !0u32;
        for &byte in data {
            crc ^= u32::from(byte);
            for _ in 0..8 {
                crc = if crc & 1 != 0 {
                    (crc >> 1) ^ poly
                } else {
                    crc >> 1
                };
            }
        }
        !crc
    }

    #[test]
    fn catalogue_check_values() {
        assert_eq!(crc16_ccitt_false(b"123456789"), 0x29B1);
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32c(b"123456789"), 0xE306_9283);
    }

    #[test]
    fn tables_agree_with_bitwise_reference() {
        let data: Vec<u8> = (0..=255u8).chain((0..=255u8).rev()).collect();
        for len in [0, 1, 2, 7, 64, data.len()] {
            let slice = &data[..len];
            assert_eq!(crc16_ccitt_false(slice), crc16_reference(slice));
            assert_eq!(crc32(slice), crc32_reference(0xEDB8_8320, slice));
            assert_eq!(crc32c(slice), crc32_reference(0x82F6_3B78, slice));
        }
    }

    #[test]
    fn trait_impls_report_width() {
        assert_eq!(Crc16CcittFalse::LEN, 2);
        assert_eq!(Crc32::LEN, 4);
        assert_eq!(Crc32c::LEN, 4);
        assert_eq!(
            Crc16CcittFalse.compute(b"a"),
            u64::from(crc16_ccitt_false(b"a"))
        );
        assert_eq!(Crc32.compute(b"a"), u64::from(crc32(b"a")));
        assert_eq!(Crc32c.compute(b"a"), u64::from(crc32c(b"a")));
    }
}
