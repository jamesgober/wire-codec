//! End-to-end smoke tests that exercise the public API together.

use wire_codec::framing::{Delimited, Endian, Framer, LengthPrefixed, LengthWidth};
use wire_codec::{varint, zigzag, Decode, Encode, ReadBuf, Result, WriteBuf};

#[test]
fn version_is_set() {
    assert!(!wire_codec::VERSION.is_empty());
}

/// User type carrying a zigzag-encoded i64, then a length-prefixed payload.
/// Demonstrates Encode + Decode + varint + zigzag composing in one struct.
#[derive(Debug, PartialEq, Eq)]
struct Record<'a> {
    delta: i64,
    payload: &'a [u8],
}

impl<'a> Encode for Record<'a> {
    fn encoded_size(&self) -> usize {
        let zz = zigzag::encode_i64(self.delta);
        varint::encoded_len_u64(zz)
            + varint::encoded_len_u64(self.payload.len() as u64)
            + self.payload.len()
    }

    fn encode(&self, buf: &mut WriteBuf<'_>) -> Result<()> {
        let zz = zigzag::encode_i64(self.delta);
        varint::encode_u64(zz, buf)?;
        varint::encode_u64(self.payload.len() as u64, buf)?;
        buf.write_bytes(self.payload)
    }
}

impl<'de> Decode<'de> for Record<'de> {
    fn decode(buf: &mut ReadBuf<'de>) -> Result<Self> {
        let delta = zigzag::decode_i64(varint::decode_u64(buf)?);
        let len = varint::decode_u64(buf)? as usize;
        let payload = buf.read_bytes(len)?;
        Ok(Self { delta, payload })
    }
}

#[test]
fn record_round_trip_through_length_prefixed_framer() {
    let record = Record {
        delta: -1234,
        payload: b"hello world",
    };

    // Encode the record into its own buffer.
    let mut record_storage = [0u8; 32];
    let mut record_buf = WriteBuf::new(&mut record_storage);
    record.encode(&mut record_buf).unwrap();
    let record_len = record_buf.position();
    let encoded_record = &record_storage[..record_len];
    assert_eq!(record_len, record.encoded_size());

    // Wrap the encoded record in a length-prefixed frame.
    let framer = LengthPrefixed::new(LengthWidth::U16, Endian::Big);
    let mut wire = [0u8; 64];
    let mut wire_buf = WriteBuf::new(&mut wire);
    framer.write_frame(encoded_record, &mut wire_buf).unwrap();
    let wire_len = wire_buf.position();

    // Decode the frame, then the record.
    let frame = framer.next_frame(&wire[..wire_len]).unwrap().unwrap();
    assert_eq!(frame.consumed(), wire_len);
    let mut read = ReadBuf::new(frame.payload());
    let decoded = Record::decode(&mut read).unwrap();
    assert!(read.is_empty());
    assert_eq!(decoded, record);
}

#[test]
fn delimited_stream_yields_consecutive_frames() {
    let framer = Delimited::new(b"\n").unwrap();

    let mut wire = [0u8; 64];
    let mut buf = WriteBuf::new(&mut wire);
    framer.write_frame(b"alpha", &mut buf).unwrap();
    framer.write_frame(b"beta", &mut buf).unwrap();
    framer.write_frame(b"gamma", &mut buf).unwrap();
    let n = buf.position();

    let mut input: &[u8] = &wire[..n];
    let mut frames: Vec<&[u8]> = Vec::new();
    while let Some(frame) = framer.next_frame(input).unwrap() {
        frames.push(frame.payload());
        input = &input[frame.consumed()..];
    }
    assert!(input.is_empty());
    assert_eq!(
        frames,
        vec![b"alpha".as_slice(), b"beta".as_slice(), b"gamma".as_slice()]
    );
}
