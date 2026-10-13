# Changelog

All notable changes to `wire-codec` will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

---

## [Unreleased]

---

## [1.1.0] — 2026-10-13

Hardening and framing release. Fixes every correctness, security, and
documentation finding of the 2026-10 audit, including a remote panic on
32-bit targets (items kept or deferred are listed in
`docs/release/v1.1.0.md`), and adds the framers, codecs, and adapters the
1.0 roadmap listed as post-1.0 work. No breaking API
changes; see "Changed" for the MSRV raise and the behaviour fixes.

### Added

- `framing::VarintPrefixed`: LEB128 length-prefixed framer (the protobuf
  length-delimited stream format). Bounded (8 MiB) and canonical by
  default; `with_max_payload`, `with_canonical`.
- `framing::Checksummed<F, C>` and `ChecksumScope`: a decorator that appends
  and verifies a checksum trailer after the frames of any framer.
- `checksum` module: the `Checksum` trait, `Crc16CcittFalse`, `Crc32`,
  `Crc32c`, and `crc16_ccitt_false` / `crc32` / `crc32c` functions
  (const-generated tables, slice-by-8 for CRC-32, no dependencies).
- `cobs` module: allocation-free COBS `encode`, `decode`, `decode_in_place`,
  and `max_encoded_len`.
- `packed_struct!` macro and `packed::PackedField` trait: bit-packed structs
  with per-field widths checked at compile time, implementing `Encode` and
  `Decode`, plus `pack` / `unpack` and `BITS` / `SIZE` constants.
- `framing::StreamDecoder<F>` and the provided trait method
  `Framer::next_frame_from`: resumable frame extraction for streaming input.
  `Delimited` resumes its search where the previous call stopped. The
  progress is cleared automatically when decoding fails, and
  `StreamDecoder::reset` / `FrameCodec::reset` clear it when the caller
  discards or replaces buffered input.
- Provided trait method `Framer::max_frame_len` (upper bound on the bytes
  `write_frame` writes); implemented by every built-in framer.
- `LengthWidth::U24` and `LengthWidth::U64`.
- `LengthPrefixed::with_length_adjustment` / `length_adjustment` for
  protocols whose length field counts more or less than the payload.
- Bounded constructors for untrusted input: `LengthPrefixed::bounded`
  (8 MiB, `LengthPrefixed::DEFAULT_MAX_PAYLOAD`) and `Delimited::bounded`
  (64 KiB, `Delimited::DEFAULT_MAX_PAYLOAD`).
- `Error::DelimiterInPayload` and `Error::ChecksumMismatch`.
- Varint helpers: `encode_u16` / `decode_u16` / `encoded_len_u16`, `usize`
  variants, zigzag `encode_i32` / `decode_i32` / `encode_i64` / `decode_i64`
  with `encoded_len_i32` / `encoded_len_i64`, and canonical decoders
  `decode_{u16,u32,u64,usize,i32,i64}_canonical` that reject overlong
  encodings with `Error::InvalidEncoding`.
- `Encode` / `Decode` impls for `i8`, `bool` (strict `0` / `1`), `[u8; N]`,
  `&'de [u8; N]` (decode), `[u8]` (encode), tuples of two to six elements,
  and the new `Be<T>`, `Le<T>`, and `Varint<T>` wrappers (every integer from
  `u16` to `i128`, `f32`, `f64`; varint for `u16`..`u64`, `usize`, `i32`,
  `i64`).
- `ReadBuf::peek_u8`, `read_array`, `read_array_ref`, `split_to`, `read_i8`,
  and big/little-endian readers for `u128`, `i16`..`i128`, `f32`, `f64`;
  matching `WriteBuf` writers.
- `BitReader::read_bool` / `read_signed_bits` and `BitWriter::write_bool` /
  `write_signed_bits`.
- `impl From<Error> for std::io::Error` (with `std`).
- Optional features, all off by default: `memchr` (SIMD delimiter search),
  `bytes` (`codec::FrameCodec` over `BytesMut`, `no_std` + `alloc`),
  `tokio-util` and `asynchronous-codec` (`Decoder` / `Encoder` impls for
  `FrameCodec`).
- `fuzz/` with cargo-fuzz targets for every decoder, sharing invariants with
  a proptest harness (`tests/fuzz_harness.rs`) that runs in `cargo test`.
- README and `docs/API.md` code blocks are compiled as doctests.
- CI: the MSRV leg now really runs 1.85 (`RUSTUP_TOOLCHAIN` overrides
  `rust-toolchain.toml`); new jobs for feature combinations, `no_std`
  (`thumbv7em-none-eabihf`) and `wasm32` builds, 32-bit tests (`i686`
  Windows and Linux), a docs.rs-style nightly build, a fuzz smoke run,
  `cargo semver-checks`, and a weekly scheduled run.
- `docs/release/v1.1.0.md`.

### Changed

- **MSRV raised from 1.75 to 1.85, edition 2021 to 2024.** This enables
  `core::error::Error` on `no_std` and current dev-dependencies. Users on
  older compilers should pin `~1.0`: only projects using MSRV-aware
  resolution (Cargo 1.84+ with `resolver = "3"` or
  `incompatible-rust-versions = "fallback"`) skip 1.1 automatically. The
  MSRV is only ever raised in a minor release.
- `Error` now implements `core::error::Error` unconditionally (previously
  `std::error::Error` only with the `std` feature; it is the same trait).
- `Delimited::write_frame` rejects a payload that the receiver would not
  read back as one frame (one containing the delimiter, or one whose tail
  merges with a self-overlapping delimiter) with
  `Error::DelimiterInPayload`. 1.0 wrote such frames silently, and the peer
  split them.
- Writes are atomic: `varint::encode_*` and every built-in framer's
  `write_frame` check room before writing, so on error the cursor is where
  it was and nothing was written. Varint decoding is atomic too (1.0
  advanced the cursor past the bytes it had read before failing).
- `Checksummed::write_frame` and tuple `Encode` impls check the output room
  before anything else so that a short buffer writes nothing. As a result
  they report `BufferFull` ahead of a payload error from the inner framer or
  element (for example `FrameTooLarge` or `DelimiterInPayload`); with
  enough room, those errors are reported as before. Bare `LengthPrefixed`,
  `VarintPrefixed`, and `Delimited` still report payload errors first.
- `Delimited` searches with a SWAR first-byte scan (about 60x faster on a
  1 KiB line) instead of a byte-by-byte slice compare.
- `FrameTooLarge.len` from `Delimited::next_frame` is `max_payload + 1`, a
  lower bound on the frame length, rather than the length of the whole
  buffered input.
- `#[must_use]` on the pure accessors of `ReadBuf`, `WriteBuf`, `Frame`, the
  framers, and `varint::encoded_len_*`.
- `#![forbid(unsafe_code)]`.
- `rust-toolchain.toml` pins `1.95.0` instead of the floating `stable`.
- `proptest` dev-dependency moves from `>=1, <1.6` to `1.11` (the newest
  release whose `rust-version` matches the MSRV); the 1.0 rationale for the
  pin ("1.6+ raises its own MSRV") was inaccurate.
- `rustfmt.toml` drops ten nightly-only options that stable ignored.
- `deny.toml` license allow-list trimmed to licenses actually present.
- Benches pass every decode input through `black_box`; the 1.0 decode
  numbers were inflated by constant folding.
- Crate keywords and description updated (the description now matches what
  ships).

### Fixed

- `LengthPrefixed::next_frame` could overflow `header_size + len` on 32-bit
  targets and then panic slicing the input (`FF FF FF FF` as a `u32`
  prefix). Lengths taken from the wire are now converted with checked
  arithmetic; an unaddressable length is `FrameTooLarge`.
- `BitWriter::align_to_byte` and `BitWriter::finish` did not clear the
  padding bits, so stale bytes from a reused buffer went out on the wire.
- `BitReader` / `BitWriter` capacity arithmetic (`len * 8`) could overflow
  on 32-bit targets for slices of 512 MiB or more.
- Examples and tests no longer truncate decoded `u64` lengths with
  `as usize`.
- README "Length-prefixed framing" example and the API.md "Hot-loop"
  cookbook example did not compile.
- README claimed the crate was "`no_std` by default" (it is `std` by
  default) and that the delimited framer's scan window was bounded by
  default (it is only with a limit).
- The "packed struct encoding" in the crate description did not exist; it
  is now `packed_struct!`.
- API.md listed `Error` without `EmptyDelimiter`, `LengthWidth` without
  `#[non_exhaustive]` and `Hash`, and called the `u8` impls a blanket impl.
- README reference-style links had no definitions.
- A test comment referred to a delimiter-in-payload test that did not
  exist; it does now.

### Security

- Remote panic (denial of service) on 32-bit targets from a hostile length
  prefix (see "Fixed").
- Quadratic CPU cost from a peer dripping a long line one byte at a time
  is now linear through `StreamDecoder` / `next_frame_from`.
- Bounded constructors and documentation for untrusted input; the new
  `VarintPrefixed` is bounded by default.
- Canonical varint decoders close a malleability gap for signed or hashed
  encodings.
- Padding bits can no longer leak earlier buffer contents.
- Fuzz targets for every decoder; each fuzzed for 60 seconds before release
  with no findings.

---

## [1.0.0] — 2026-05-21

### Added

- Substantially expanded `README.md`: design rationale section, feature
  list, four usage patterns (length-prefixed, delimited, varint + zigzag,
  bit-packed records), reference performance table, stability statement,
  feature-flag table, and project layout diagram.
- Cookbook section in `docs/API.md` with four recipes: append-only stream
  over a growing buffer, fixed-output buffer with `BufferFull`
  back-pressure, length-prefixed envelope around a structured record, and
  pre-sized-output hot-loop pattern.
- Stability note in `docs/API.md` introducing the 1.0 freeze contract.
- Crate-level rustdoc in `src/lib.rs` rewritten with a "When to reach for
  this crate" section, module map table, stability section, and feature
  flag table.
- `docs/release/v1.0.0.md` with the locked benchmark baseline.

### Changed

- `README.md` corrects the edition claim (`2024` -> `2021`) and bumps the
  dependency version pin to `"1"`.
- `docs/API.md` `VERSION` reference updated to `"1.0.0"`.

### Stability

- Public API surface is now frozen. Subsequent `1.x` releases will be
  additive: new `Error` variants, new `LengthWidth` variants, new methods
  on existing types, new feature flags. Any breaking change requires a
  `2.0` release.

---

## [0.9.0] — 2026-05-21

### Added

- `Error::EmptyDelimiter` variant returned by `Delimited::new` when the
  delimiter is empty.
- `#[non_exhaustive]` on `LengthWidth` so wider prefix widths can be added
  in future minor releases.
- `PartialEq`, `Eq`, `Hash` derives on `LengthPrefixed`, `Delimited`,
  `LengthWidth`, and `Endian`.
- `#[must_use]` on `Encode::encoded_size`.
- `# Errors` doc sections on every `Result`-returning function in
  `buf`, `varint`, `bitfield`, and `framing`.
- `rust-toolchain.toml` pinning the toolchain channel for reproducible
  builds.
- `deny.toml` policy file for `cargo deny` (license allow-list, banned
  crates, duplicate version warning, registry source restriction).
- `supply-chain` CI job running `cargo audit` and `cargo deny check` on
  every push and pull request.
- Canonical `REPS.md` vendored from the workspace standard.
- `docs/release/v0.9.0.md` audit report covering every section of REPS.

### Changed

- **Breaking:** `framing::Delimited::new(delimiter)` now returns
  `Result<Delimited<'_>, Error>` instead of `Self`. An empty delimiter
  yields `Err(Error::EmptyDelimiter)` rather than panicking. Update call
  sites with `.unwrap()` or `?` as appropriate.
- Crate-level lint set expanded to the REPS-mandated full configuration
  (`#![deny(warnings)]`, `#![deny(clippy::unreachable)]`).

### Fixed

- `Delimited::new` no longer panics on empty input. This was the only
  panic point in the shipping code paths.

### Security

- `cargo audit` and `cargo deny` are now part of CI. Any RustSec
  advisory, banned crate, or license violation in the dependency tree
  blocks the merge.

---

## [0.5.0] — 2026-05-21

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

## [0.2.0] — 2026-05-21

### Added

- Public error type `Error` with `Result<T>` alias.
- Zero-copy cursor primitives `ReadBuf` and `WriteBuf` for borrowed byte slices.
- Codec trait pair `Encode` and `Decode<'de>` with an impl for `u8`.
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

## [0.1.0] — 2026-05-18

### Added

- Initial scaffold and repository bootstrap.
- REPS compliance baseline.
- CI for Linux/macOS/Windows on stable and MSRV (1.75).

[Unreleased]: https://github.com/jamesgober/wire-codec/compare/v1.1.0...HEAD
[1.1.0]: https://github.com/jamesgober/wire-codec/compare/v1.0.0...v1.1.0
[1.0.0]: https://github.com/jamesgober/wire-codec/compare/v0.9.0...v1.0.0
[0.9.0]: https://github.com/jamesgober/wire-codec/compare/v0.5.0...v0.9.0
[0.5.0]: https://github.com/jamesgober/wire-codec/compare/v0.2.0...v0.5.0
[0.2.0]: https://github.com/jamesgober/wire-codec/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/jamesgober/wire-codec/releases/tag/v0.1.0