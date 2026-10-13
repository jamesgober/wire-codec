//! libFuzzer target: see `harness::bitfield` for the invariants checked.
#![no_main]

use libfuzzer_sys::fuzz_target;

#[path = "../../tests/support/fuzz_harness.rs"]
mod harness;

fuzz_target!(|data: &[u8]| harness::bitfield(data));
