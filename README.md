<h1 align="center">
    <strong>wire-codec</strong>
    <br>
    <sup><sub>BINARY FRAMING AND ENCODING PRIMITIVES</sub></sup>
</h1>

<p align="center">
    <a href="https://crates.io/crates/wire-codec"><img alt="crates.io" src="https://img.shields.io/crates/v/wire-codec.svg"></a>
    <a href="https://docs.rs/wire-codec"><img alt="docs.rs" src="https://docs.rs/wire-codec/badge.svg"></a>
    <a href="https://github.com/jamesgober/wire-codec/actions/workflows/ci.yml"><img alt="CI" src="https://github.com/jamesgober/wire-codec/actions/workflows/ci.yml/badge.svg"></a>
    <a href="#license"><img alt="license" src="https://img.shields.io/badge/license-Apache--2.0%20OR%20MIT-blue.svg"></a>
</p>

<p align="center">Length-prefixed, delimiter-based, and custom framing strategies. Built-in varint, zigzag, bitfield, and packed-struct encoding. Runtime-agnostic.</p>

---

## Status

**Active development.** Scaffolded and on the path to 1.0. See [.dev/ROADMAP.md](.dev/ROADMAP.md) for milestone tracking.

The public API is not yet stable. Pin specific versions; expect changes pre-1.0.

---

## What it does

Binary frame codec and protocol codec toolkit. Length-prefixed, delimiter-based, and custom framing strategies. Built-in varint, zigzag, bitfield, and packed struct encoding. Runtime-agnostic foundation under network-protocol crates.

---

## Quick start

```toml
[dependencies]
wire-codec = "0.1"
```

---

## Standards

- **REPS** governs every decision. See [REPS.md](REPS.md).
- **MSRV:** Rust 1.75.
- **Edition:** 2024.
- **Cross-platform:** Linux, macOS, Windows.

---

## License

Dual-licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE))
- MIT License ([LICENSE-MIT](LICENSE-MIT))

at your option.

---

<sub>Copyright &copy; 2026 <strong>James Gober</strong>. All rights reserved.</sub>