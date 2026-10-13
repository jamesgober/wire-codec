//! Tests for the optional `tokio-util` and `asynchronous-codec` adapters.
//!
//! The trait methods are called directly on `BytesMut` buffers, which is
//! exactly what `Framed` does internally, so no async runtime is needed.

#![cfg(any(feature = "tokio-util", feature = "asynchronous-codec"))]

use std::io;

use bytes::BytesMut;
use wire_codec::Error;
use wire_codec::codec::FrameCodec;
use wire_codec::framing::{Delimited, Endian, LengthPrefixed, LengthWidth};

fn wire_error(err: &io::Error) -> Option<Error> {
    err.get_ref()
        .and_then(|e| e.downcast_ref::<Error>())
        .copied()
}

#[cfg(feature = "tokio-util")]
mod tokio {
    use super::*;
    use bytes::Bytes;
    use tokio_util::codec::{Decoder, Encoder};
    use wire_codec::checksum::Crc32;
    use wire_codec::framing::Checksummed;

    #[test]
    fn length_prefixed_round_trip_through_traits() {
        let mut codec = FrameCodec::new(LengthPrefixed::bounded(LengthWidth::U32, Endian::Big));
        let mut wire = BytesMut::new();
        Encoder::<&[u8]>::encode(&mut codec, b"one", &mut wire).unwrap();
        Encoder::<Bytes>::encode(&mut codec, Bytes::from_static(b"two"), &mut wire).unwrap();
        Encoder::<BytesMut>::encode(&mut codec, BytesMut::from(&b"three"[..]), &mut wire).unwrap();

        let mut got = Vec::new();
        while let Some(frame) = codec.decode(&mut wire).unwrap() {
            got.push(frame.freeze());
        }
        assert_eq!(got, [&b"one"[..], b"two", b"three"]);
        assert_eq!(codec.decode_eof(&mut wire).unwrap(), None);
    }

    #[test]
    fn errors_surface_as_io_errors() {
        let mut codec = FrameCodec::new(Checksummed::new(
            Delimited::bounded(b"\n").unwrap(),
            Crc32,
            Endian::Little,
        ));
        let mut wire = BytesMut::new();
        let err = Encoder::<&[u8]>::encode(&mut codec, b"a\nb", &mut wire).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        assert_eq!(wire_error(&err), Some(Error::DelimiterInPayload));

        Encoder::<&[u8]>::encode(&mut codec, b"ok", &mut wire).unwrap();
        let last = wire.len() - 1;
        wire[last] ^= 0xFF;
        let err = codec.decode(&mut wire).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        assert_eq!(wire_error(&err), Some(Error::ChecksumMismatch));
    }

    #[test]
    fn partial_frame_at_eof_is_an_error() {
        let mut codec = FrameCodec::new(Delimited::bounded(b"\n").unwrap());
        let mut wire = BytesMut::from(&b"no newline"[..]);
        assert_eq!(codec.decode(&mut wire).unwrap(), None);
        let err = codec.decode_eof(&mut wire).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::UnexpectedEof);
    }
}

#[cfg(feature = "asynchronous-codec")]
mod futures {
    use super::*;
    use asynchronous_codec::{Decoder, Encoder};

    #[test]
    fn delimited_round_trip_through_traits() {
        let mut codec = FrameCodec::new(Delimited::bounded(b"\r\n").unwrap());
        let mut wire = BytesMut::new();
        codec.encode(b"HELLO", &mut wire).unwrap();
        codec.encode(b"WORLD", &mut wire).unwrap();
        assert_eq!(&wire[..], b"HELLO\r\nWORLD\r\n");
        assert_eq!(&codec.decode(&mut wire).unwrap().unwrap()[..], b"HELLO");
        assert_eq!(&codec.decode_eof(&mut wire).unwrap().unwrap()[..], b"WORLD");
        assert_eq!(codec.decode_eof(&mut wire).unwrap(), None);
    }

    #[test]
    fn oversized_frame_is_rejected() {
        let mut codec =
            FrameCodec::new(LengthPrefixed::new(LengthWidth::U16, Endian::Big).with_max_payload(4));
        let mut wire = BytesMut::from(&[0x00, 0x05][..]);
        let err = codec.decode(&mut wire).unwrap_err();
        assert_eq!(
            wire_error(&err),
            Some(Error::FrameTooLarge { len: 5, limit: 4 })
        );
        let err = codec.encode(b"12345", &mut wire).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
        let err = codec
            .decode_eof(&mut BytesMut::from(&[0x00][..]))
            .unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::UnexpectedEof);
    }
}
