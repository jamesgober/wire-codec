//! Encode a record made of a zigzag-varint signed delta, a varint sensor id,
//! and a length-prefixed payload. Read it back and verify equality.
//!
//! Run with `cargo run --example varint_record`.

use wire_codec::{varint, zigzag, Decode, Encode, ReadBuf, Result, WriteBuf};

#[derive(Debug, PartialEq, Eq)]
struct Sample<'a> {
    delta_ns: i64,
    sensor_id: u32,
    payload: &'a [u8],
}

impl<'a> Encode for Sample<'a> {
    fn encoded_size(&self) -> usize {
        varint::encoded_len_u64(zigzag::encode_i64(self.delta_ns))
            + varint::encoded_len_u32(self.sensor_id)
            + varint::encoded_len_u64(self.payload.len() as u64)
            + self.payload.len()
    }

    fn encode(&self, buf: &mut WriteBuf<'_>) -> Result<()> {
        varint::encode_u64(zigzag::encode_i64(self.delta_ns), buf)?;
        varint::encode_u32(self.sensor_id, buf)?;
        varint::encode_u64(self.payload.len() as u64, buf)?;
        buf.write_bytes(self.payload)
    }
}

impl<'de> Decode<'de> for Sample<'de> {
    fn decode(buf: &mut ReadBuf<'de>) -> Result<Self> {
        let delta_ns = zigzag::decode_i64(varint::decode_u64(buf)?);
        let sensor_id = varint::decode_u32(buf)?;
        let len = varint::decode_u64(buf)? as usize;
        let payload = buf.read_bytes(len)?;
        Ok(Self {
            delta_ns,
            sensor_id,
            payload,
        })
    }
}

fn main() {
    let sample = Sample {
        delta_ns: -750,
        sensor_id: 42,
        payload: b"temp=21.4 humidity=55",
    };

    let mut wire = [0u8; 64];
    let mut writer = WriteBuf::new(&mut wire);
    sample
        .encode(&mut writer)
        .expect("buffer sized for the demo");
    let written = writer.position();
    println!("encoded {written} bytes: {:02X?}", &wire[..written]);
    println!("encoded_size advertised: {}", sample.encoded_size());

    let mut reader = ReadBuf::new(&wire[..written]);
    let decoded = Sample::decode(&mut reader).expect("round-trips");
    assert!(reader.is_empty());
    assert_eq!(decoded, sample);
    println!("decoded: {decoded:?}");
}
