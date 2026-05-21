//! Length-prefixed echo: encode three messages, then read them back as frames.
//!
//! Run with `cargo run --example length_prefixed_echo`.

use wire_codec::framing::{Endian, Framer, LengthPrefixed, LengthWidth};
use wire_codec::WriteBuf;

fn main() {
    let framer = LengthPrefixed::new(LengthWidth::U16, Endian::Big);
    let messages: [&[u8]; 3] = [b"hello", b"wire-codec", b"goodbye"];

    let mut wire = [0u8; 128];
    let mut writer = WriteBuf::new(&mut wire);
    for msg in &messages {
        framer
            .write_frame(msg, &mut writer)
            .expect("buffer sized for the demo");
    }
    let written = writer.position();
    println!("encoded {written} bytes across {} frames", messages.len());

    let mut input = &wire[..written];
    let mut index = 0;
    while let Some(frame) = framer.next_frame(input).expect("valid wire") {
        let payload = std::str::from_utf8(frame.payload()).expect("ascii payload");
        println!(
            "frame {index}: {payload:?} (consumed {} bytes)",
            frame.consumed()
        );
        input = &input[frame.consumed()..];
        index += 1;
    }
}
