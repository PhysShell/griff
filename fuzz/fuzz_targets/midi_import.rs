#![no_main]

//! Fuzz target: `griff_core::midi::import` (P0, ADR-0010).
//!
//! Oracle — for any byte slice, `import` must:
//!   * not panic (a panic is a libFuzzer crash);
//!   * not hang (enforced by libFuzzer `-timeout`);
//!   * not allocate without bound (enforced by libFuzzer `-rss_limit_mb`
//!     and `-malloc_limit_mb`);
//!   * return either `Ok(MidiSong)` or a typed `MidiError` — never any
//!     other failure mode.
//!
//! The first regression seed, `corpus/midi_import/hang_ppqn1_eighth.mid`,
//! is a known-failing input: a PPQN=1 / 1-8 file drives
//! `group_into_bars` into a non-advancing loop. It is committed unfixed on
//! the planning branch (see `docs/fuzzing.md`).

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = griff_core::midi::import(data);
});
