//! `packed_struct!` used from a downstream crate with strict lints, the way
//! a REPS-compliant consumer would.

#![deny(warnings, missing_docs, unused_results, clippy::all)]

use wire_codec::framing::{Framer, VarintPrefixed};
use wire_codec::{Decode, Encode, Error, ReadBuf, WriteBuf, packed_struct};

packed_struct! {
    /// A 16-bit sensor header.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct SensorHeader {
        /// Protocol version.
        pub version: u8 = 3,
        /// Set when the reading is calibrated.
        pub calibrated: bool = 1,
        /// Signed temperature delta.
        pub delta: i8 = 5,
        /// Channel number.
        pub channel: u16 = 7,
    }
}

packed_struct! {
    /// Private type with a trailing comma and no derives.
    struct Bare {
        a: u8 = 8,
    }
}

#[test]
fn packed_struct_inside_a_varint_frame() {
    let header = SensorHeader {
        version: 5,
        calibrated: true,
        delta: -16,
        channel: 127,
    };
    assert_eq!(SensorHeader::BITS, 16);
    assert_eq!(SensorHeader::SIZE, 2);
    assert_eq!(header.pack().unwrap(), [0b1011_1000, 0b0111_1111]);

    let mut record = [0u8; SensorHeader::SIZE];
    header.encode(&mut WriteBuf::new(&mut record)).unwrap();

    let framer = VarintPrefixed::new();
    let mut wire = [0u8; 8];
    let mut out = WriteBuf::new(&mut wire);
    framer.write_frame(&record, &mut out).unwrap();
    let n = out.position();

    let frame = framer.next_frame(&wire[..n]).unwrap().unwrap();
    let decoded = SensorHeader::decode(&mut ReadBuf::new(frame.payload())).unwrap();
    assert_eq!(decoded, header);
}

#[test]
fn out_of_range_field_is_rejected() {
    let header = SensorHeader {
        version: 8,
        calibrated: false,
        delta: 0,
        channel: 0,
    };
    let mut out = [0u8; 2];
    assert_eq!(
        header.encode(&mut WriteBuf::new(&mut out)),
        Err(Error::BitOverflow)
    );
}

#[test]
fn bare_struct_round_trips() {
    let bytes = Bare { a: 0x5A }.pack().unwrap();
    assert_eq!(Bare::unpack(&bytes).unwrap().a, 0x5A);
}
