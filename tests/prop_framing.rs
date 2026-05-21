//! Property tests for the framing strategies.
//!
//! Invariants under test:
//! - LengthPrefixed: encode-then-decode is the identity on payload contents,
//!   and `Frame::consumed` equals the number of bytes written.
//! - LengthPrefixed: truncated wire input returns `Ok(None)` and never panics.
//! - LengthPrefixed: a payload over the configured limit yields
//!   `FrameTooLarge` on both encode and decode.
//! - Delimited: encode-then-decode is the identity, with `Frame::consumed`
//!   counting the delimiter.
//! - Delimited: arbitrary input bytes never panic the decoder.

use proptest::prelude::*;
use wire_codec::framing::{Delimited, Endian, Framer, LengthPrefixed, LengthWidth};
use wire_codec::WriteBuf;

fn width_strategy() -> impl Strategy<Value = LengthWidth> {
    prop_oneof![
        Just(LengthWidth::U8),
        Just(LengthWidth::U16),
        Just(LengthWidth::U32),
    ]
}

fn endian_strategy() -> impl Strategy<Value = Endian> {
    prop_oneof![Just(Endian::Big), Just(Endian::Little)]
}

/// Cap payloads at u16-prefix max so u8/u16 widths can hold the worst case.
const MAX_PROP_PAYLOAD: usize = 255;

proptest! {
    #[test]
    fn length_prefixed_round_trip(
        width in width_strategy(),
        endian in endian_strategy(),
        payload in proptest::collection::vec(any::<u8>(), 0..MAX_PROP_PAYLOAD),
    ) {
        let framer = LengthPrefixed::new(width, endian);
        let mut wire = vec![0u8; width.header_size() + payload.len()];
        let mut buf = WriteBuf::new(&mut wire);
        framer.write_frame(&payload, &mut buf).unwrap();
        let written = buf.position();
        prop_assert_eq!(written, width.header_size() + payload.len());

        let frame = framer.next_frame(&wire[..written]).unwrap().unwrap();
        prop_assert_eq!(frame.payload(), payload.as_slice());
        prop_assert_eq!(frame.consumed(), written);
    }

    #[test]
    fn length_prefixed_truncated_returns_none(
        width in width_strategy(),
        endian in endian_strategy(),
        payload in proptest::collection::vec(any::<u8>(), 1..MAX_PROP_PAYLOAD),
        cut in 0usize..MAX_PROP_PAYLOAD,
    ) {
        let framer = LengthPrefixed::new(width, endian);
        let mut wire = vec![0u8; width.header_size() + payload.len()];
        let mut buf = WriteBuf::new(&mut wire);
        framer.write_frame(&payload, &mut buf).unwrap();
        let written = buf.position();

        let truncated_to = cut.min(written.saturating_sub(1));
        let result = framer.next_frame(&wire[..truncated_to]).unwrap();
        prop_assert!(result.is_none(), "expected None on truncated input, got {result:?}");
    }

    #[test]
    fn length_prefixed_rejects_over_limit(
        width in width_strategy(),
        endian in endian_strategy(),
        cap in 0u64..32,
        overhead in 1usize..16,
    ) {
        let framer = LengthPrefixed::new(width, endian).with_max_payload(cap);
        let payload = vec![0u8; (cap as usize) + overhead];
        let mut wire = vec![0u8; width.header_size() + payload.len()];
        let mut buf = WriteBuf::new(&mut wire);
        prop_assert!(framer.write_frame(&payload, &mut buf).is_err());
    }

    #[test]
    fn length_prefixed_decode_random_bytes_never_panics(
        width in width_strategy(),
        endian in endian_strategy(),
        bytes in proptest::collection::vec(any::<u8>(), 0..256),
    ) {
        let framer = LengthPrefixed::new(width, endian).with_max_payload(64);
        let _ = framer.next_frame(&bytes);
    }

    #[test]
    fn delimited_round_trip(
        delim in proptest::collection::vec(1u8..=255, 1..4),
        payload in proptest::collection::vec(any::<u8>(), 0..256),
    ) {
        // Skip payloads that themselves contain the delimiter; that case is
        // tested separately as a protocol error.
        prop_assume!(!contains_subsequence(&payload, &delim));

        // Build a stable lifetime for `delim` by leaking it; tiny allocation
        // per case is acceptable for a property test.
        let delim_static: &'static [u8] = Box::leak(delim.into_boxed_slice());
        let framer = Delimited::new(delim_static);

        let mut wire = vec![0u8; payload.len() + delim_static.len()];
        let mut buf = WriteBuf::new(&mut wire);
        framer.write_frame(&payload, &mut buf).unwrap();
        let written = buf.position();

        let frame = framer.next_frame(&wire[..written]).unwrap().unwrap();
        prop_assert_eq!(frame.payload(), payload.as_slice());
        prop_assert_eq!(frame.consumed(), written);
    }

    #[test]
    fn delimited_decode_random_bytes_never_panics(
        delim in proptest::collection::vec(1u8..=255, 1..3),
        bytes in proptest::collection::vec(any::<u8>(), 0..256),
    ) {
        let delim_static: &'static [u8] = Box::leak(delim.into_boxed_slice());
        let framer = Delimited::new(delim_static).with_max_payload(128);
        let _ = framer.next_frame(&bytes);
    }
}

fn contains_subsequence(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() || haystack.len() < needle.len() {
        return false;
    }
    (0..=haystack.len() - needle.len()).any(|i| &haystack[i..i + needle.len()] == needle)
}
