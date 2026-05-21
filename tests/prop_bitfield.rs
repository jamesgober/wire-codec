//! Property tests for bitfield cursors.
//!
//! Invariants under test:
//! - Round-trip: a sequence of `(value, width)` writes round-trips through a
//!   matching read sequence for any combination of widths.
//! - `BitWriter::finish` reports the correct whole-byte count.
//! - Random byte input never panics the reader.
//! - Value width validation: writing a value with bits above the requested
//!   width returns `BitOverflow`.

use proptest::prelude::*;
use wire_codec::bitfield::MAX_BIT_WIDTH;
use wire_codec::{BitReader, BitWriter, Error};

#[derive(Debug, Clone)]
struct Field {
    value: u64,
    width: u32,
}

fn field_strategy() -> impl Strategy<Value = Field> {
    (1u32..=MAX_BIT_WIDTH).prop_flat_map(|width| {
        let value_strategy = if width == 64 {
            any::<u64>().boxed()
        } else {
            (0u64..(1u64 << width)).boxed()
        };
        value_strategy.prop_map(move |value| Field { value, width })
    })
}

fn fields_strategy() -> impl Strategy<Value = Vec<Field>> {
    proptest::collection::vec(field_strategy(), 0..16)
        .prop_filter("sum of widths must fit in a u32", |fields| {
            fields.iter().map(|f| u64::from(f.width)).sum::<u64>() <= u64::from(u32::MAX)
        })
}

proptest! {
    #[test]
    fn bitfield_round_trip(fields in fields_strategy()) {
        let total_bits: usize = fields.iter().map(|f| f.width as usize).sum();
        let byte_capacity = total_bits.div_ceil(8).max(1);
        let mut storage = vec![0u8; byte_capacity];

        let mut w = BitWriter::new(&mut storage);
        for f in &fields {
            w.write_bits(f.value, f.width).unwrap();
        }
        let bytes_written = w.finish();
        prop_assert_eq!(bytes_written, total_bits.div_ceil(8));

        let mut r = BitReader::new(&storage[..bytes_written]);
        for f in &fields {
            prop_assert_eq!(r.read_bits(f.width).unwrap(), f.value);
        }
    }

    #[test]
    fn bitfield_rejects_value_overflow(width in 1u32..63, extra in 1u64..=64) {
        // Constructing a value with at least one bit set above `width`.
        let oversized = (1u64 << width) | extra;
        let mut storage = [0u8; 16];
        let mut w = BitWriter::new(&mut storage);
        let result = w.write_bits(oversized, width);
        prop_assert!(matches!(result, Err(Error::BitOverflow)));
    }

    #[test]
    fn bitfield_reader_random_input_never_panics(
        bytes in proptest::collection::vec(any::<u8>(), 0..32),
        widths in proptest::collection::vec(1u32..=64, 0..16),
    ) {
        let mut r = BitReader::new(&bytes);
        for w in widths {
            if r.read_bits(w).is_err() {
                break;
            }
        }
    }
}
