//! Shared fuzz harness.
//!
//! Each function takes arbitrary bytes, drives one decoder with them, and
//! panics if an invariant is violated. The same functions run under
//! libFuzzer (`fuzz/fuzz_targets/*.rs`, via `cargo fuzz`) and under proptest
//! in `tests/fuzz_harness.rs`, so the invariants are exercised by a plain
//! `cargo test` on every platform, including ones where libFuzzer is not
//! available.
//!
//! Configuration (widths, delimiters, limits) is taken from the first few
//! input bytes so a single byte stream explores every framer setting.

#![allow(dead_code)]

use wire_codec::checksum::{Crc16CcittFalse, Crc32};
use wire_codec::framing::{
    Checksummed, Delimited, Endian, Framer, LengthPrefixed, LengthWidth, StreamDecoder,
    VarintPrefixed,
};
use wire_codec::{BitReader, BitWriter, Decode, Encode, Error, ReadBuf, WriteBuf, cobs, varint};

const WIDTHS: [LengthWidth; 5] = [
    LengthWidth::U8,
    LengthWidth::U16,
    LengthWidth::U24,
    LengthWidth::U32,
    LengthWidth::U64,
];

fn split_config(data: &[u8], n: usize) -> (&[u8], &[u8]) {
    if data.len() < n {
        (data, &[])
    } else {
        data.split_at(n)
    }
}

fn byte(config: &[u8], i: usize) -> u8 {
    config.get(i).copied().unwrap_or(0)
}

/// Every varint decoder: no panics, atomic on error, consistent on success.
pub fn varint(data: &[u8]) {
    macro_rules! check {
        ($decode:path, $canonical:path, $encode:path, $len:path) => {{
            let mut lenient = ReadBuf::new(data);
            match $decode(&mut lenient) {
                Ok(value) => {
                    let used = lenient.position();
                    assert!(used >= 1 && used <= data.len());
                    assert!($len(value) <= used);
                    let mut strict = ReadBuf::new(data);
                    match $canonical(&mut strict) {
                        Ok(v) => {
                            assert_eq!(v, value);
                            assert_eq!(strict.position(), used);
                            // Canonical bytes are exactly what the encoder writes.
                            let mut out = [0u8; varint::MAX_LEN_U64];
                            let mut w = WriteBuf::new(&mut out);
                            $encode(value, &mut w).unwrap();
                            assert_eq!(w.written(), &data[..used]);
                        }
                        Err(e) => {
                            assert_eq!(e, Error::InvalidEncoding);
                            assert_eq!(strict.position(), 0);
                        }
                    }
                }
                Err(e) => {
                    assert!(matches!(e, Error::UnexpectedEof | Error::VarintOverflow));
                    assert_eq!(lenient.position(), 0, "decode must be atomic");
                }
            }
        }};
    }
    check!(
        varint::decode_u16,
        varint::decode_u16_canonical,
        varint::encode_u16,
        varint::encoded_len_u16
    );
    check!(
        varint::decode_u32,
        varint::decode_u32_canonical,
        varint::encode_u32,
        varint::encoded_len_u32
    );
    check!(
        varint::decode_u64,
        varint::decode_u64_canonical,
        varint::encode_u64,
        varint::encoded_len_u64
    );
    check!(
        varint::decode_usize,
        varint::decode_usize_canonical,
        varint::encode_usize,
        varint::encoded_len_usize
    );
    check!(
        varint::decode_i32,
        varint::decode_i32_canonical,
        varint::encode_i32,
        varint::encoded_len_i32
    );
    check!(
        varint::decode_i64,
        varint::decode_i64_canonical,
        varint::encode_i64,
        varint::encoded_len_i64
    );
}

/// Check the generic `Framer` contract on one decode result, then make sure
/// the framer re-encodes the payload into exactly the bytes it consumed when
/// `unique` says the wire format has a single encoding per payload.
fn check_frame<F: Framer>(framer: &F, input: &[u8], unique: bool) {
    let Ok(Some(frame)) = framer.next_frame(input) else {
        return;
    };
    let consumed = frame.consumed();
    assert!(consumed >= 1 && consumed <= input.len());
    assert!(frame.payload().len() <= consumed);
    let base = input.as_ptr() as usize;
    let start = frame.payload().as_ptr() as usize - base;
    assert!(
        start + frame.payload().len() <= consumed,
        "payload lies inside the frame"
    );
    if unique {
        let mut out = vec![0u8; consumed];
        let mut w = WriteBuf::new(&mut out);
        framer.write_frame(frame.payload(), &mut w).unwrap();
        assert_eq!(w.written(), &input[..consumed]);
    }
}

/// Decode a whole buffer frame by frame, both in one call per frame and by
/// feeding it one byte at a time through a `StreamDecoder`. The resumable
/// scan must find exactly the same frames.
fn check_streaming<F: Framer + Clone>(framer: &F, input: &[u8]) {
    let mut batch = Vec::new();
    let mut rest = input;
    let batch_err = loop {
        match framer.next_frame(rest) {
            Ok(Some(frame)) => {
                batch.push(frame.payload().to_vec());
                rest = &rest[frame.consumed()..];
            }
            Ok(None) => break None,
            Err(e) => break Some(e),
        }
    };

    let mut decoder = StreamDecoder::new(framer.clone());
    let mut streamed = Vec::new();
    let mut start = 0;
    let mut stream_err = None;
    'feed: for end in 0..=input.len() {
        loop {
            match decoder.decode(&input[start..end]) {
                Ok(Some(frame)) => {
                    streamed.push(frame.payload().to_vec());
                    start += frame.consumed();
                }
                Ok(None) => break,
                Err(e) => {
                    stream_err = Some(e);
                    break 'feed;
                }
            }
        }
    }
    assert_eq!(batch, streamed, "streaming and batch decoding disagree");
    assert_eq!(batch_err.is_some(), stream_err.is_some());
}

pub fn length_prefixed(data: &[u8]) {
    let (config, input) = split_config(data, 3);
    let width = WIDTHS[usize::from(byte(config, 0)) % WIDTHS.len()];
    let endian = if byte(config, 0) & 0x80 == 0 {
        Endian::Big
    } else {
        Endian::Little
    };
    let max = u64::from(byte(config, 1)) * 3;
    let adjustment = i64::from(byte(config, 2) as i8);
    let framer = LengthPrefixed::new(width, endian)
        .with_max_payload(max)
        .with_length_adjustment(adjustment);
    check_frame(&framer, input, true);
    check_streaming(&framer, input);
    // Unbounded and hostile prefixes must not panic either.
    let _ = LengthPrefixed::new(width, endian).next_frame(input);
    let _ = LengthPrefixed::new(width, endian)
        .with_length_adjustment(i64::MAX)
        .next_frame(input);
    let _ = LengthPrefixed::new(width, endian)
        .with_length_adjustment(i64::MIN)
        .next_frame(input);
}

pub fn delimited(data: &[u8]) {
    let (config, input) = split_config(data, 4);
    let delim_len = 1 + usize::from(byte(config, 0)) % 3;
    let delimiter = &config[1.min(config.len())..(1 + delim_len).min(config.len())];
    let Ok(framer) = Delimited::new(delimiter) else {
        return;
    };
    let framer = framer.with_max_payload(usize::from(byte(config, 0) >> 2));
    check_frame(&framer, input, true);
    check_streaming(&framer, input);
    check_streaming(&Delimited::new(delimiter).unwrap(), input);

    // Writing the raw input as a payload either round-trips or is rejected
    // because it contains the delimiter.
    let mut out = vec![0u8; input.len() + delimiter.len()];
    let mut w = WriteBuf::new(&mut out);
    let unbounded = Delimited::new(delimiter).unwrap();
    match unbounded.write_frame(input, &mut w) {
        Ok(()) => {
            let n = w.position();
            let frame = unbounded.next_frame(&out[..n]).unwrap().unwrap();
            assert_eq!(frame.payload(), input);
            assert_eq!(frame.consumed(), n);
        }
        Err(e) => {
            assert_eq!(e, Error::DelimiterInPayload);
            assert_eq!(w.position(), 0);
        }
    }
}

pub fn varint_prefixed(data: &[u8]) {
    let (config, input) = split_config(data, 1);
    let canonical = byte(config, 0) & 1 == 0;
    let framer = VarintPrefixed::new()
        .with_canonical(canonical)
        .with_max_payload(usize::from(byte(config, 0)) * 4);
    check_frame(&framer, input, canonical);
    check_streaming(&framer, input);
    let _ = VarintPrefixed::new()
        .with_max_payload(usize::MAX)
        .next_frame(input);
}

pub fn checksummed(data: &[u8]) {
    let (config, input) = split_config(data, 1);
    let endian = if byte(config, 0) & 1 == 0 {
        Endian::Big
    } else {
        Endian::Little
    };
    let crc16 = Checksummed::new(
        LengthPrefixed::new(LengthWidth::U8, endian),
        Crc16CcittFalse,
        endian,
    );
    check_frame(&crc16, input, true);
    check_streaming(&crc16, input);
    let crc32 = Checksummed::new(Delimited::bounded(b"\n").unwrap(), Crc32, endian);
    check_frame(&crc32, input, true);
    check_streaming(&crc32, input);

    // Any payload round-trips through the length-prefixed variant.
    let payload = &input[..input.len().min(255)];
    let mut out = vec![0u8; payload.len() + 3];
    let mut w = WriteBuf::new(&mut out);
    crc16.write_frame(payload, &mut w).unwrap();
    let n = w.position();
    assert_eq!(
        crc16.next_frame(&out[..n]).unwrap().unwrap().payload(),
        payload
    );
}

pub fn bitfield(data: &[u8]) {
    let (config, input) = split_config(data, 8);
    let mut reader = BitReader::new(input);
    let mut fields = Vec::new();
    for (i, &w) in config.iter().enumerate().cycle().take(64) {
        let width = u32::from(w % 65);
        let before = reader.bits_consumed();
        let result = if i % 3 == 0 {
            reader.read_signed_bits(width).map(|v| (v as u64, true))
        } else {
            reader.read_bits(width).map(|v| (v, false))
        };
        match result {
            Ok((value, signed)) => fields.push((value, width, signed)),
            Err(e) => {
                assert!(matches!(e, Error::BitOverflow | Error::UnexpectedEof));
                assert_eq!(reader.bits_consumed(), before, "read_bits must be atomic");
            }
        }
        if i % 7 == 6 {
            reader.align_to_byte();
            fields.push((0, 0, false));
        }
    }
    // Writing the same fields back and replaying the reads yields the same
    // values. (Bits skipped by `align_to_byte` are not compared: the reader
    // discards them and the writer zeroes them.)
    let mut out = vec![0xA5u8; input.len()];
    let mut writer = BitWriter::new(&mut out);
    for &(value, width, signed) in &fields {
        if width == 0 {
            writer.align_to_byte().unwrap();
        } else if signed {
            writer.write_signed_bits(value as i64, width).unwrap();
        } else {
            writer.write_bits(value, width).unwrap();
        }
    }
    assert_eq!(writer.bits_written(), reader.bits_consumed());
    let bytes = writer.finish();
    let mut replay = BitReader::new(&out[..bytes]);
    for &(value, width, signed) in &fields {
        if width == 0 {
            replay.align_to_byte();
        } else if signed {
            assert_eq!(replay.read_signed_bits(width).unwrap() as u64, value);
        } else {
            assert_eq!(replay.read_bits(width).unwrap(), value);
        }
    }
    // Trailing padding in the last byte is zero.
    let tail = replay.bits_remaining();
    if tail > 0 {
        assert_eq!(replay.read_bits(tail as u32).unwrap(), 0);
    }
}

pub fn cobs(data: &[u8]) {
    // Encode then decode is the identity, and the encoding has no zeros.
    let mut encoded = vec![0u8; cobs::max_encoded_len(data.len())];
    let mut w = WriteBuf::new(&mut encoded);
    cobs::encode(data, &mut w).unwrap();
    let n = w.position();
    assert!(!encoded[..n].contains(&0));
    let mut decoded = vec![0u8; n];
    let mut w = WriteBuf::new(&mut decoded);
    cobs::decode(&encoded[..n], &mut w).unwrap();
    assert_eq!(w.written(), data);

    // Decoding arbitrary bytes never panics, and the two decoders agree.
    let mut out = vec![0u8; data.len()];
    let mut w = WriteBuf::new(&mut out);
    let copied = cobs::decode(data, &mut w).map(|()| w.written().to_vec());
    let mut in_place = data.to_vec();
    let in_place = cobs::decode_in_place(&mut in_place).map(|len| in_place[..len].to_vec());
    assert_eq!(copied, in_place);
    if copied.is_err() {
        assert_eq!(copied, Err(Error::InvalidEncoding));
    }
}

wire_codec::packed_struct! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    struct Packed {
        a: bool = 1,
        b: u8 = 5,
        c: i16 = 11,
        d: u32 = 19,
        e: i64 = 33,
    }
}

pub fn packed(data: &[u8]) {
    let mut r = ReadBuf::new(data);
    match Packed::decode(&mut r) {
        Ok(value) => {
            assert_eq!(r.position(), Packed::SIZE);
            let mut out = [0u8; Packed::SIZE];
            value.encode(&mut WriteBuf::new(&mut out)).unwrap();
            assert_eq!(&out[..], &data[..Packed::SIZE]);
        }
        Err(e) => {
            assert!(matches!(e, Error::UnexpectedEof | Error::InvalidEncoding));
            assert_eq!(r.position(), 0);
        }
    }
}
