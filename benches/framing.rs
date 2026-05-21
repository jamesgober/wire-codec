//! Manual benchmark harness for the framing strategies.
//!
//! Run with `cargo bench --bench framing`. See `benches/codec.rs` for notes on
//! the harness format.

use std::hint::black_box;
use std::time::Instant;

use wire_codec::framing::{Delimited, Endian, Framer, LengthPrefixed, LengthWidth};
use wire_codec::WriteBuf;

const ITERATIONS: usize = 2_000_000;

fn time_loop<F: FnMut()>(name: &str, iterations: usize, mut body: F) {
    for _ in 0..(iterations / 10).max(1) {
        body();
    }
    let start = Instant::now();
    for _ in 0..iterations {
        body();
    }
    let elapsed = start.elapsed();
    let per_op = elapsed.as_nanos() as f64 / iterations as f64;
    println!("  {name:<52} {per_op:>8.2} ns/op  ({iterations} iters)");
}

fn bench_length_prefixed_write() {
    let framer = LengthPrefixed::new(LengthWidth::U16, Endian::Big);
    let payload = vec![0xAAu8; 64];
    let mut wire = vec![0u8; 2 + payload.len()];

    time_loop("LengthPrefixed::write_frame (64 B)", ITERATIONS, || {
        let mut buf = WriteBuf::new(&mut wire);
        framer
            .write_frame(black_box(&payload), &mut buf)
            .expect("sized for the payload");
        black_box(buf.position());
    });
}

fn bench_length_prefixed_read() {
    let framer = LengthPrefixed::new(LengthWidth::U16, Endian::Big);
    let payload = vec![0xAAu8; 64];
    let mut wire = vec![0u8; 2 + payload.len()];
    {
        let mut buf = WriteBuf::new(&mut wire);
        framer
            .write_frame(&payload, &mut buf)
            .expect("sized for the payload");
    }

    time_loop("LengthPrefixed::next_frame (64 B)", ITERATIONS, || {
        let frame = framer
            .next_frame(black_box(&wire))
            .expect("ok")
            .expect("complete");
        black_box(frame.payload());
    });
}

fn bench_delimited_scan_short() {
    let framer = Delimited::new(b"\n").expect("non-empty delimiter");
    let wire = b"short line\nrest of the buffer that we never reach";
    time_loop("Delimited::next_frame (short line)", ITERATIONS, || {
        let frame = framer
            .next_frame(black_box(wire))
            .expect("ok")
            .expect("complete");
        black_box(frame.consumed());
    });
}

fn bench_delimited_scan_long() {
    let framer = Delimited::new(b"\r\n").expect("non-empty delimiter");
    let mut wire = vec![b'A'; 1024];
    wire.extend_from_slice(b"\r\nbody");
    time_loop(
        "Delimited::next_frame (1 KiB scan + CRLF)",
        ITERATIONS,
        || {
            let frame = framer
                .next_frame(black_box(&wire))
                .expect("ok")
                .expect("complete");
            black_box(frame.consumed());
        },
    );
}

fn main() {
    println!("wire-codec benches/framing.rs");
    println!("  iterations per scenario: {ITERATIONS}");
    println!();

    println!("[length-prefixed]");
    bench_length_prefixed_write();
    bench_length_prefixed_read();

    println!();
    println!("[delimited]");
    bench_delimited_scan_short();
    bench_delimited_scan_long();
}
