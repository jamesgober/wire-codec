//! Integration tests against realistic protocol shapes.
//!
//! These tests stitch together the public API the way a downstream consumer
//! would and exercise it against full message flows, not just single calls.

use wire_codec::framing::{Delimited, Endian, Framer, LengthPrefixed, LengthWidth};
use wire_codec::{varint, zigzag, Decode, Encode, ReadBuf, Result, WriteBuf};

// ---- A stream of length-prefixed messages, decoded one at a time --------

#[test]
fn length_prefixed_stream_with_partial_delivery() {
    let framer = LengthPrefixed::new(LengthWidth::U16, Endian::Big);
    let messages: [&[u8]; 4] = [b"first", b"second message", b"", b"x"];

    // Encode the whole stream into one wire buffer.
    let mut wire = vec![0u8; 256];
    let mut w = WriteBuf::new(&mut wire);
    for msg in &messages {
        framer.write_frame(msg, &mut w).unwrap();
    }
    let wire_len = w.position();
    let wire = &wire[..wire_len];

    // Simulate a transport delivering the bytes in arbitrary chunks. After
    // every chunk we attempt to read as many complete frames as are present.
    let chunk_sizes = [1, 3, 5, 7, 11, 13, 17, 23, 100];
    let mut delivered: Vec<u8> = Vec::new();
    let mut received: Vec<Vec<u8>> = Vec::new();
    let mut cursor = 0;
    for &chunk in &chunk_sizes {
        let end = (cursor + chunk).min(wire.len());
        delivered.extend_from_slice(&wire[cursor..end]);
        cursor = end;

        let mut read_at = 0usize;
        while let Some(frame) = framer.next_frame(&delivered[read_at..]).unwrap() {
            received.push(frame.payload().to_vec());
            read_at += frame.consumed();
        }
        delivered.drain(..read_at);

        if cursor == wire.len() {
            break;
        }
    }

    assert_eq!(received.len(), messages.len());
    for (got, &want) in received.iter().zip(messages.iter()) {
        assert_eq!(got, want);
    }
    assert!(delivered.is_empty(), "no bytes should remain unparsed");
}

// ---- A line protocol: newline-delimited text commands -------------------

#[test]
fn newline_protocol_parses_commands() {
    let framer = Delimited::new(b"\n").with_max_payload(1024);
    let transcript = b"PING\nECHO hello world\nQUIT\n";

    let mut input: &[u8] = transcript;
    let mut commands: Vec<&[u8]> = Vec::new();
    while let Some(frame) = framer.next_frame(input).unwrap() {
        commands.push(frame.payload());
        input = &input[frame.consumed()..];
    }

    assert_eq!(
        commands,
        vec![b"PING".as_slice(), b"ECHO hello world", b"QUIT"]
    );
    assert!(input.is_empty());
}

// ---- A CR/LF protocol: HTTP-style request line then headers --------------

#[test]
fn crlf_protocol_extracts_request_line_and_headers() {
    let framer = Delimited::new(b"\r\n");
    let request = b"GET /index.html HTTP/1.1\r\nHost: example.com\r\nUser-Agent: wire-codec/0.5\r\n\r\nbody-bytes";

    let mut input: &[u8] = request;
    let request_line = framer.next_frame(input).unwrap().unwrap();
    assert_eq!(request_line.payload(), b"GET /index.html HTTP/1.1");
    input = &input[request_line.consumed()..];

    let mut headers: Vec<&[u8]> = Vec::new();
    while let Some(frame) = framer.next_frame(input).unwrap() {
        if frame.payload().is_empty() {
            input = &input[frame.consumed()..];
            break;
        }
        headers.push(frame.payload());
        input = &input[frame.consumed()..];
    }

    assert_eq!(
        headers,
        vec![
            b"Host: example.com".as_slice(),
            b"User-Agent: wire-codec/0.5"
        ]
    );
    assert_eq!(input, b"body-bytes");
}

// ---- A length-prefixed binary record with varint and zigzag fields -------

#[derive(Debug, PartialEq, Eq)]
struct Event<'a> {
    timestamp_delta: i64,
    sensor_id: u32,
    payload: &'a [u8],
}

impl<'a> Encode for Event<'a> {
    fn encoded_size(&self) -> usize {
        varint::encoded_len_u64(zigzag::encode_i64(self.timestamp_delta))
            + varint::encoded_len_u32(self.sensor_id)
            + varint::encoded_len_u64(self.payload.len() as u64)
            + self.payload.len()
    }

    fn encode(&self, buf: &mut WriteBuf<'_>) -> Result<()> {
        varint::encode_u64(zigzag::encode_i64(self.timestamp_delta), buf)?;
        varint::encode_u32(self.sensor_id, buf)?;
        varint::encode_u64(self.payload.len() as u64, buf)?;
        buf.write_bytes(self.payload)
    }
}

impl<'de> Decode<'de> for Event<'de> {
    fn decode(buf: &mut ReadBuf<'de>) -> Result<Self> {
        let timestamp_delta = zigzag::decode_i64(varint::decode_u64(buf)?);
        let sensor_id = varint::decode_u32(buf)?;
        let len = varint::decode_u64(buf)? as usize;
        let payload = buf.read_bytes(len)?;
        Ok(Self {
            timestamp_delta,
            sensor_id,
            payload,
        })
    }
}

#[test]
fn event_record_pipeline() {
    let events = [
        Event {
            timestamp_delta: 0,
            sensor_id: 1,
            payload: b"boot",
        },
        Event {
            timestamp_delta: -50,
            sensor_id: 42,
            payload: b"temp=21.4",
        },
        Event {
            timestamp_delta: 12_345,
            sensor_id: 999_999,
            payload: b"",
        },
    ];

    let framer = LengthPrefixed::new(LengthWidth::U16, Endian::Big);

    // Encode every event into a varint-prefixed record, then wrap each in a
    // length-prefixed frame. Two layers of framing, which is how real
    // protocols composed of nested codecs work.
    let mut wire = vec![0u8; 1024];
    let mut w = WriteBuf::new(&mut wire);
    for ev in &events {
        let mut record_storage = vec![0u8; ev.encoded_size()];
        let mut rw = WriteBuf::new(&mut record_storage);
        ev.encode(&mut rw).unwrap();
        let record_len = rw.position();
        framer
            .write_frame(&record_storage[..record_len], &mut w)
            .unwrap();
    }
    let wire_len = w.position();

    // Read them back.
    let mut input: &[u8] = &wire[..wire_len];
    let mut decoded: Vec<Event<'_>> = Vec::new();
    while let Some(frame) = framer.next_frame(input).unwrap() {
        let payload = frame.payload();
        let mut rb = ReadBuf::new(payload);
        let event = Event::decode(&mut rb).unwrap();
        assert!(
            rb.is_empty(),
            "decoder must consume the whole frame payload"
        );
        decoded.push(event);
        input = &input[frame.consumed()..];
    }

    assert!(input.is_empty());
    assert_eq!(decoded.len(), events.len());
    for (got, want) in decoded.iter().zip(events.iter()) {
        assert_eq!(got, want);
    }
}

// ---- Verifying back-pressure semantics on the framer ---------------------

#[test]
fn framer_signals_more_bytes_needed_without_consuming() {
    let framer = LengthPrefixed::new(LengthWidth::U32, Endian::Big);

    // Build a payload of 100 bytes wrapped in its frame.
    let payload = vec![0xAAu8; 100];
    let mut wire = vec![0u8; 4 + payload.len()];
    let mut w = WriteBuf::new(&mut wire);
    framer.write_frame(&payload, &mut w).unwrap();
    let total = w.position();

    // Feeding any prefix below the complete frame must yield Ok(None).
    for prefix_len in 0..total {
        let result = framer.next_frame(&wire[..prefix_len]).unwrap();
        assert!(
            result.is_none(),
            "expected None for prefix_len={prefix_len}, got {result:?}"
        );
    }

    // The full input yields the frame with consumed == total.
    let frame = framer.next_frame(&wire[..total]).unwrap().unwrap();
    assert_eq!(frame.consumed(), total);
    assert_eq!(frame.payload(), payload.as_slice());
}
