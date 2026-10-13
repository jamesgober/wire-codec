//! Property tests for the varint module.
//!
//! Invariants under test:
//! - Round-trip: `decode(encode(v)) == v` for every `u32` and `u64`.
//! - Size agreement: `encode` writes exactly `encoded_len(v)` bytes.
//! - Decode never reads past the encoded prefix.
//! - Truncated input always returns `UnexpectedEof`, never panics.
//! - Overlong / overflow input always returns `VarintOverflow`, never panics.
//! - Decode is atomic: on error the cursor has not moved.
//! - Canonical decoding accepts exactly the encoder's output.

use proptest::prelude::*;
use wire_codec::{Error, ReadBuf, WriteBuf, varint};

proptest! {
    #[test]
    fn varint_u32_round_trip(value in any::<u32>()) {
        let mut storage = [0u8; varint::MAX_LEN_U32];
        let mut w = WriteBuf::new(&mut storage);
        varint::encode_u32(value, &mut w).unwrap();
        let written = w.position();
        prop_assert_eq!(written, varint::encoded_len_u32(value));

        let mut r = ReadBuf::new(&storage[..written]);
        prop_assert_eq!(varint::decode_u32(&mut r).unwrap(), value);
        prop_assert!(r.is_empty(), "decoder must consume every byte");
    }

    #[test]
    fn varint_u64_round_trip(value in any::<u64>()) {
        let mut storage = [0u8; varint::MAX_LEN_U64];
        let mut w = WriteBuf::new(&mut storage);
        varint::encode_u64(value, &mut w).unwrap();
        let written = w.position();
        prop_assert_eq!(written, varint::encoded_len_u64(value));

        let mut r = ReadBuf::new(&storage[..written]);
        prop_assert_eq!(varint::decode_u64(&mut r).unwrap(), value);
        prop_assert!(r.is_empty(), "decoder must consume every byte");
    }

    #[test]
    fn varint_u32_truncation_never_panics(value in any::<u32>()) {
        let mut storage = [0u8; varint::MAX_LEN_U32];
        let mut w = WriteBuf::new(&mut storage);
        varint::encode_u32(value, &mut w).unwrap();
        let written = w.position();

        // For every truncation point shorter than the full encoding the decoder
        // must yield UnexpectedEof. We only truncate when the last present byte
        // still has its continuation bit set, otherwise the prefix is a valid
        // shorter varint of a different value.
        for cut in 0..written {
            if cut > 0 && storage[cut - 1] & 0x80 == 0 {
                continue;
            }
            let mut r = ReadBuf::new(&storage[..cut]);
            let result = varint::decode_u32(&mut r);
            prop_assert!(matches!(result, Err(Error::UnexpectedEof)), "cut={cut} value={value} got={result:?}");
        }
    }

    #[test]
    fn varint_u32_decode_random_bytes_never_panics(bytes in proptest::collection::vec(any::<u8>(), 0..16)) {
        // Any byte sequence must produce Ok or a structured Err. No panics, no
        // out-of-bounds reads, no infinite loops.
        // On error the cursor must not move; on success it moves by 1..=5.
        let mut r = ReadBuf::new(&bytes);
        match varint::decode_u32(&mut r) {
            Ok(_) => prop_assert!((1..=varint::MAX_LEN_U32).contains(&r.position())),
            Err(_) => prop_assert_eq!(r.position(), 0),
        }
    }

    #[test]
    fn varint_u64_decode_random_bytes_never_panics(bytes in proptest::collection::vec(any::<u8>(), 0..32)) {
        let mut r = ReadBuf::new(&bytes);
        match varint::decode_u64(&mut r) {
            Ok(_) => prop_assert!((1..=varint::MAX_LEN_U64).contains(&r.position())),
            Err(_) => prop_assert_eq!(r.position(), 0),
        }
    }

    #[test]
    fn varint_u32_overlong_input_overflows(extra in 1usize..16) {
        // MAX_LEN_U32 + extra bytes of continuation must overflow, not panic.
        let mut bytes = vec![0x80u8; varint::MAX_LEN_U32 + extra];
        let last = bytes.len() - 1;
        bytes[last] = 0x01;
        let mut r = ReadBuf::new(&bytes);
        prop_assert_eq!(varint::decode_u32(&mut r), Err(Error::VarintOverflow));
    }

    #[test]
    fn varint_canonical_accepts_exactly_encoder_output(value in any::<u64>(), pad in 1usize..4) {
        let mut storage = [0u8; varint::MAX_LEN_U64];
        let mut w = WriteBuf::new(&mut storage);
        varint::encode_u64(value, &mut w).unwrap();
        let n = w.position();
        prop_assert_eq!(varint::decode_u64_canonical(&mut ReadBuf::new(&storage[..n])), Ok(value));

        // Pad with redundant continuation bytes while staying within ten.
        if n + pad <= varint::MAX_LEN_U64 {
            let mut padded = storage[..n].to_vec();
            padded[n - 1] |= 0x80;
            padded.extend(core::iter::repeat_n(0x80u8, pad - 1));
            padded.push(0x00);
            prop_assert_eq!(varint::decode_u64(&mut ReadBuf::new(&padded)), Ok(value));
            let mut r = ReadBuf::new(&padded);
            prop_assert_eq!(varint::decode_u64_canonical(&mut r), Err(Error::InvalidEncoding));
            prop_assert_eq!(r.position(), 0);
        }
    }

    #[test]
    fn varint_signed_round_trip(value in any::<i64>()) {
        let mut storage = [0u8; varint::MAX_LEN_U64];
        let mut w = WriteBuf::new(&mut storage);
        varint::encode_i64(value, &mut w).unwrap();
        let n = w.position();
        prop_assert_eq!(n, varint::encoded_len_i64(value));
        prop_assert_eq!(varint::decode_i64(&mut ReadBuf::new(&storage[..n])), Ok(value));
    }
}
