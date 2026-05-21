//! Manual benchmark harness for the codec primitives.
//!
//! Run with `cargo bench --bench codec`. Each scenario reports throughput as
//! nanoseconds per operation across a configurable iteration count.
//!
//! This harness has zero external dependencies on purpose. It is not a
//! statistical tool; for fine-grained measurements, plug in `criterion`
//! externally when MSRV permits.

use std::hint::black_box;
use std::time::Instant;

use wire_codec::{varint, zigzag, ReadBuf, WriteBuf};

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
        let mut buf = ReadBuf::new(&bytes[..2]);
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
        let mut buf = ReadBuf::new(&bytes_max);
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
        let mut buf = ReadBuf::new(&bytes);
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
        let mut buf = ReadBuf::new(&storage);
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

    println!();
    println!("[zigzag]");
    bench_zigzag();

    println!();
    println!("[buf primitives]");
    bench_readbuf_primitives();
    bench_writebuf_primitives();
}
