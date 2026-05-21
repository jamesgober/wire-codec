# Changelog

All notable changes to `wire-codec` will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [Unreleased]

### Added

### Changed

### Fixed

### Security

---

## [0.5.0] - 2026-05-21

### Added

- Property-test suite (`proptest`) covering varint round-trip, zigzag inverse,
  framer round-trip, bitfield round-trip, and panic-freedom on arbitrary byte
  input.
- Integration test suite (`tests/integration_protocols.rs`) covering:
  partial-delivery length-prefixed streams, newline-delimited line protocols,
  HTTP-style CR/LF request parsing, two-layer length-prefixed + varint record
  pipelines, and back-pressure semantics.
- Bench harness with zero-dependency manual timing (`benches/codec.rs`,
  `benches/framing.rs`). Reports nanoseconds per operation for the varint,
  zigzag, buffer, and framing primitives.
- Three runnable examples under `examples/`: `length_prefixed_echo`,
  `newline_protocol`, `varint_record`.
- `proptest = ">=1, <1.6"` as a dev-dependency (capped below 1.6 so the
  `cargo test` build resolves on MSRV 1.75).
- `.github` CI cache step updated to `actions/cache@v5` to retire the Node 20
  deprecation warning.
- `.gitattributes` pinning all text files to LF on checkout, so Windows CI
  matches `rustfmt.toml`'s `newline_style = "Unix"`.

### Changed

- Documentation surface is unchanged; existing rustdoc covered the new
  invariants without revision.

### Fixed

- `clippy.toml`: removed `allow-panic-in-tests`, `allow-dbg-in-tests`, and
  `allow-print-in-tests` (added after MSRV 1.75); kept the two pre-1.75
  options (`allow-unwrap-in-tests`, `allow-expect-in-tests`).
- `varint::encoded_len_*` allow attributes now use
  `#[allow(unknown_lints, clippy::manual_div_ceil)]` so older clippy releases
  silently skip the unknown lint instead of failing.

---

## [0.2.0] - 2026-05-21

### Added

- Public error type `Error` with `Result<T>` alias.
- Zero-copy cursor primitives `ReadBuf` and `WriteBuf` for borrowed byte slices.
- Codec trait pair `Encode` and `Decode<'de>` with a blanket impl for `u8`.
- LEB128 unsigned varint module (`varint::{encode_u32, encode_u64, decode_u32, decode_u64, encoded_len_u32, encoded_len_u64, MAX_LEN_U16, MAX_LEN_U32, MAX_LEN_U64}`).
- Zigzag signed-integer module (`zigzag::{encode_i32, decode_i32, encode_i64, decode_i64}`), all `const fn`.
- Packed-bit cursors `BitReader` and `BitWriter`, big-endian (MSB-first) bit order.
- Framing module with `Framer` trait, `Frame<'a>` envelope, `LengthPrefixed`
  (u8 / u16 / u32 width, big or little endian), and `Delimited` (single or
  multi-byte delimiter).
- End-to-end smoke test exercising encode + framing + decode in one pipeline.
- API reference at `docs/API.md`.
- Release note at `docs/release/v0.2.0.md`.

### Changed

- Edition lowered from 2024 to 2021 to match the declared MSRV of 1.75.

---

## [0.1.0] - 2026-05-18

### Added

- Initial scaffold and repository bootstrap.
- REPS compliance baseline.
- CI for Linux/macOS/Windows on stable and MSRV (1.75).

[Unreleased]: https://github.com/jamesgober/wire-codec/compare/v0.5.0...HEAD
[0.5.0]: https://github.com/jamesgober/wire-codec/compare/v0.2.0...v0.5.0
[0.2.0]: https://github.com/jamesgober/wire-codec/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/jamesgober/wire-codec/releases/tag/v0.1.0