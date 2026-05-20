#![no_main]

//! Fuzz target: MIDI import -> export -> re-import (P0 invariants at S2,
//! ADR-0010).
//!
//! Flow: try `import(data)`; if it yields a song, `export` it and import the
//! exported bytes again.
//!
//! Oracle (normalized invariants — byte-identical roundtrip is explicitly
//! NOT required):
//!   * no panic / hang / unbounded allocation (libFuzzer limits);
//!   * a successfully imported song re-imports after export;
//!   * PPQN is preserved across the roundtrip;
//!   * the note-bearing track count is preserved across the roundtrip.
//!
//! Richer normalized invariants (bar-duration validity, pitch/velocity in
//! range, no reversed ranges, no duration overflow) are added in S2 once the
//! canonical model and `LossReport` exist.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(song) = griff_core::midi::import(data) else {
        return;
    };
    // A typed export error is an acceptable outcome, not an oracle failure.
    let Ok(bytes) = griff_core::midi::export(&song) else {
        return;
    };
    let reimported =
        griff_core::midi::import(&bytes).expect("exported MIDI must re-import");
    assert_eq!(song.ppqn, reimported.ppqn, "roundtrip must preserve PPQN");
    assert_eq!(
        song.tracks.len(),
        reimported.tracks.len(),
        "roundtrip must preserve the note-bearing track count"
    );
});
