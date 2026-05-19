# wire-codec - Project Prompt

## Priority order

1. `REPS.md` - SUPREME AUTHORITY
2. `.dev/DIRECTIVES.md`
3. This file (`.dev/PROMPT.md`)
4. `.dev/ROADMAP.md`

## What this crate is

Binary frame codec and protocol codec toolkit. Length-prefixed, delimiter-based, and custom framing strategies. Built-in varint, zigzag, bitfield, and packed struct encoding. Runtime-agnostic foundation under network-protocol crates.

## Why it exists

Length-prefixed, delimiter-based, and custom framing strategies. Built-in varint, zigzag, bitfield, and packed-struct encoding. Runtime-agnostic.

## Skill areas

- binary encoding
- framing protocols
- varint encoding
- zero-copy parsing

## Scope (1.0)

Defined in `.dev/ROADMAP.md`.

## Out of scope (always)

- Features requiring async runtime hard-dependency
- Features pulling in heavy framework dependency
- Features that violate REPS

## Pre-1.0 audit (mandatory)

See `.dev/ROADMAP.md` for the audit checklist. Must verify:

- Feature completeness vs. the roadmap
- API accuracy and stability
- Code cleanliness (no dead code, no commented-out blocks, no TODOs)
- Error hardening
- Documentation completeness
- Test coverage
- Benchmark coverage
- Cross-platform CI passing

## Versioning

Fast-track. No slow-stepping:

- 0.1.0 - scaffold
- 0.2.0 - first real implementation
- 0.5.0 - most features in place
- 0.9.0 - feature-complete, hardening
- 0.9.x - audit findings
- 1.0.0 - stable