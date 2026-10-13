//! Manual benchmark harness for the codec primitives.
//!
//! Run with `cargo bench --bench codec`. Each scenario reports throughput as
//! nanoseconds per operation across a configurable iteration count.
//!
//! This harness has zero external dependencies on purpose. It is not a
//! statistical tool; for fine-grained measurements, plug in `criterion`
//! externally. Every decode input goes through `black_box` so the compiler
//! cannot constant-fold the work away.

use std::hint::black_box;
use std::time::Instant;

use wire_codec::{ReadBuf, WriteBuf, checksum, cobs, varint, zigzag};

const ITERATIONS: usize = 5_000_000;

fn time_loop<F: FnMut()>(name: &str, iterations: usize, mut body: F) {
    // Warm-up.
    for _ in 0..(iterations / 10).max(1) {
        body();
    }
    let start = Instant::now();
    for _ in 0..iterations {
        body();
    }
    let elapsed = start.elapsed();
    let per_op = elapsed.as_nanos() as f64 / iterations as f64;
    println!("  {name:<48} {per_op:>8.2} ns/op  ({iterations} iters)");
}

fn bench_varint_encode_u32() {
    let mut storage = [0u8; varint::MAX_LEN_U32];
    time_loop("varint::encode_u32 (300)", ITERATIONS, || {
        let mut buf = WriteBuf::new(&mut storage);
        varint::encode_u32(black_box(300), &mut buf).expect("storage is sized");
        black_box(buf.position());
    });
    time_loop("varint::encode_u32 (u32::MAX)", ITERATIONS, || {
        let mut buf = WriteBuf::new(&mut storage);
        varint::encode_u32(black_box(u32::MAX), &mut buf).expect("storage is sized");
        black_box(buf.position());
    });
}

fn bench_varint_decode_u32() {
    let mut storage = [0u8; varint::MAX_LEN_U32];
    {
        let mut buf = WriteBuf::new(&mut storage);
        varint::encode_u32(300, &mut buf).expect("storage is sized");
    }
    let bytes = storage;
    time_loop("varint::decode_u32 (2 bytes)", ITERATIONS, || {
        let mut buf = ReadBuf::new(black_box(&bytes[..2]));
        let value = varint::decode_u32(&mut buf).expect("valid encoding");
        black_box(value);
    });

    let mut storage_max = [0u8; varint::MAX_LEN_U32];
    {
        let mut buf = WriteBuf::new(&mut storage_max);
        varint::encode_u32(u32::MAX, &mut buf).expect("storage is sized");
    }
    let bytes_max = storage_max;
    time_loop("varint::decode_u32 (5 bytes)", ITERATIONS, || {
        let mut buf = ReadBuf::new(black_box(&bytes_max));
        let value = varint::decode_u32(&mut buf).expect("valid encoding");
        black_box(value);
    });
}

fn bench_varint_u64() {
    let mut storage = [0u8; varint::MAX_LEN_U64];
    {
        let mut buf = WriteBuf::new(&mut storage);
        varint::encode_u64(u64::MAX, &mut buf).expect("storage is sized");
    }
    let bytes = storage;
    time_loop("varint::decode_u64 (10 bytes)", ITERATIONS, || {
        let mut buf = ReadBuf::new(black_box(&bytes));
        let value = varint::decode_u64(&mut buf).expect("valid encoding");
        black_box(value);
    });

    let mut out = [0u8; varint::MAX_LEN_U64];
    time_loop("varint::encode_u64 (u64::MAX)", ITERATIONS, || {
        let mut buf = WriteBuf::new(&mut out);
        varint::encode_u64(black_box(u64::MAX), &mut buf).expect("storage is sized");
        black_box(buf.position());
    });
}

fn bench_varint_stream() {
    // 4096 values with a realistic length mix (mostly 1-2 bytes, some long),
    // decoded back to back. Reported per value.
    let values: Vec<u64> = (0..4096u64)
        .map(|i| match i % 8 {
            0..=3 => i % 128,
            4 | 5 => 128 + i * 31,
            6 => i << 20,
            _ => u64::MAX - i,
        })
        .collect();
    let mut wire = vec![0u8; values.len() * varint::MAX_LEN_U64];
    let mut w = WriteBuf::new(&mut wire);
    for &v in &values {
        varint::encode_u64(v, &mut w).expect("sized");
    }
    let n = w.position();
    let count = values.len();
    let rounds = 2_000;
    let start = Instant::now();
    for _ in 0..rounds {
        let mut r = ReadBuf::new(black_box(&wire[..n]));
        for _ in 0..count {
            black_box(varint::decode_u64(&mut r).expect("valid"));
        }
    }
    let per_value = start.elapsed().as_nanos() as f64 / (rounds * count) as f64;
    println!(
        "  {:<48} {per_value:>8.2} ns/op  ({rounds} x {count} values)",
        "varint::decode_u64 (mixed 4096-value stream)"
    );
}

fn bench_checksums() {
    let data = vec![0x5Au8; 1024];
    time_loop(
        "checksum::crc16_ccitt_false (1 KiB)",
        ITERATIONS / 50,
        || {
            black_box(checksum::crc16_ccitt_false(black_box(&data)));
        },
    );
    time_loop("checksum::crc32 (1 KiB)", ITERATIONS / 50, || {
        black_box(checksum::crc32(black_box(&data)));
    });
    time_loop("checksum::crc32c (1 KiB)", ITERATIONS / 50, || {
        black_box(checksum::crc32c(black_box(&data)));
    });
}

fn bench_cobs() {
    let data: Vec<u8> = (0..1024u32).map(|i| (i % 7) as u8).collect();
    let mut encoded = vec![0u8; cobs::max_encoded_len(data.len())];
    let mut decoded = vec![0u8; data.len()];
    let n = {
        let mut w = WriteBuf::new(&mut encoded);
        cobs::encode(&data, &mut w).expect("sized");
        w.position()
    };
    let mut scratch = vec![0u8; encoded.len()];
    time_loop(
        "cobs::encode (1 KiB, zero every 7th byte)",
        ITERATIONS / 50,
        || {
            let mut w = WriteBuf::new(&mut scratch);
            cobs::encode(black_box(&data), &mut w).expect("sized");
            black_box(w.position());
        },
    );
    time_loop("cobs::decode (1 KiB)", ITERATIONS / 50, || {
        let mut w = WriteBuf::new(&mut decoded);
        cobs::decode(black_box(&encoded[..n]), &mut w).expect("valid");
        black_box(w.position());
    });
}

fn bench_zigzag() {
    time_loop("zigzag::encode_i32 (-1)", ITERATIONS, || {
        black_box(zigzag::encode_i32(black_box(-1)));
    });
    time_loop("zigzag::encode_i64 (i64::MIN)", ITERATIONS, || {
        black_box(zigzag::encode_i64(black_box(i64::MIN)));
    });
}

fn bench_readbuf_primitives() {
    let storage = [0u8; 64];
    time_loop("ReadBuf::read_u32_be", ITERATIONS, || {
        let mut buf = ReadBuf::new(black_box(&storage));
        for _ in 0..16 {
            black_box(buf.read_u32_be().expect("64 bytes is enough"));
        }
    });
}

fn bench_writebuf_primitives() {
    let mut storage = [0u8; 64];
    time_loop("WriteBuf::write_u32_be", ITERATIONS, || {
        let mut buf = WriteBuf::new(&mut storage);
        for _ in 0..16 {
            buf.write_u32_be(black_box(0xDEADBEEF))
                .expect("64 bytes is enough");
        }
        black_box(buf.position());
    });
}

fn main() {
    println!("wire-codec benches/codec.rs");
    println!("  iterations per scenario: {ITERATIONS}");
    println!();

    println!("[varint]");
    bench_varint_encode_u32();
    bench_varint_decode_u32();
    bench_varint_u64();
    bench_varint_stream();

    println!();
    println!("[zigzag]");
    bench_zigzag();

    println!();
    println!("[buf primitives]");
    bench_readbuf_primitives();
    bench_writebuf_primitives();

    println!();
    println!("[checksum + cobs]");
    bench_checksums();
    bench_cobs();
}
