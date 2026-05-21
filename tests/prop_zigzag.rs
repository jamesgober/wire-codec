//! Property tests for zigzag encoding.
//!
//! Invariants:
//! - `decode(encode(v)) == v` for every `i32` and `i64`.
//! - Zigzag preserves order in magnitude: `|v1| <= |v2|` implies
//!   `encode(v1) <= encode(v2)` for non-pathological cases (this is the
//!   property that makes zigzag worth pairing with varint).
//! - The mapping is a bijection: the encoded space is exactly the range of the
//!   unsigned target type.

use proptest::prelude::*;
use wire_codec::zigzag;

proptest! {
    #[test]
    fn zigzag_i32_round_trip(value in any::<i32>()) {
        prop_assert_eq!(zigzag::decode_i32(zigzag::encode_i32(value)), value);
    }

    #[test]
    fn zigzag_i64_round_trip(value in any::<i64>()) {
        prop_assert_eq!(zigzag::decode_i64(zigzag::encode_i64(value)), value);
    }

    #[test]
    fn zigzag_i32_u32_round_trip(encoded in any::<u32>()) {
        prop_assert_eq!(zigzag::encode_i32(zigzag::decode_i32(encoded)), encoded);
    }

    #[test]
    fn zigzag_i64_u64_round_trip(encoded in any::<u64>()) {
        prop_assert_eq!(zigzag::encode_i64(zigzag::decode_i64(encoded)), encoded);
    }

    #[test]
    fn zigzag_i32_small_magnitude_stays_small(value in -1_000i32..1_000) {
        // The whole point of zigzag: small-magnitude signed values produce
        // small-magnitude unsigned codes, so the varint that follows is short.
        let encoded = zigzag::encode_i32(value);
        prop_assert!(encoded < 2_001, "value={value} encoded={encoded}");
    }
}
