//! Newline-delimited command parser, the way a small REPL or text protocol
//! would consume a TCP byte stream.
//!
//! Run with `cargo run --example newline_protocol`.

use wire_codec::framing::{Delimited, Framer};

fn main() {
    // Pretend this came off the network. Two complete commands and a partial
    // third command with no trailing newline.
    let transcript = b"PING\nECHO hello world\nQUI";
    let framer = Delimited::new(b"\n")
        .expect("non-empty delimiter")
        .with_max_payload(1024);

    let mut input: &[u8] = transcript;
    let mut frame_index = 0usize;
    while let Some(frame) = framer.next_frame(input).expect("well-formed input") {
        let line = std::str::from_utf8(frame.payload()).expect("ascii");
        println!("command {frame_index}: {line:?}");
        input = &input[frame.consumed()..];
        frame_index += 1;
    }
    println!(
        "{} bytes still buffered (no trailing delimiter yet)",
        input.len()
    );
}
