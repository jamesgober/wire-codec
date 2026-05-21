//! Property tests for the varint module.
//!
//! Invariants under test:
//! - Round-trip: `decode(encode(v)) == v` for every `u32` and `u64`.
//! - Size agreement: `encode` writes exactly `encoded_len(v)` bytes.
//! - Decode never reads past the encoded prefix.
//! - Truncated input always returns `UnexpectedEof`, never panics.
//! - Overlong / overflow input always returns `VarintOverflow`, never panics.

use proptest::prelude::*;
use wire_codec::{varint, Error, ReadBuf, WriteBuf};

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
        let mut r = ReadBuf::new(&bytes);
        let _ = varint::decode_u32(&mut r);
    }

    #[test]
    fn varint_u64_decode_random_bytes_never_panics(bytes in proptest::collection::vec(any::<u8>(), 0..32)) {
        let mut r = ReadBuf::new(&bytes);
        let _ = varint::decode_u64(&mut r);
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
}
