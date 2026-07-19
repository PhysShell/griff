#![no_main]

//! Fuzz target: MIDI import -> export -> re-import (P0 invariants at S2,
//! ADR-0010).
//!
//! Flow: try `import_score(data)`; if it yields a score, `export_score` it and
//! import the exported bytes again.
//!
//! Oracle (normalized invariants — byte-identical roundtrip is explicitly
//! NOT required):
//!   * no panic / hang / unbounded allocation (libFuzzer limits);
//!   * a successfully imported score re-imports after export;
//!   * PPQN is preserved across the roundtrip;
//!   * the note-bearing track count is preserved across the roundtrip.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(score) = griff_core::midi::import_score(data) else {
        return;
    };
    // A typed export error is an acceptable outcome, not an oracle failure.
    let Ok(export) = griff_core::midi::export_score(&score) else {
        return;
    };
    let reimported =
        griff_core::midi::import_score(&export.bytes).expect("exported MIDI must re-import");
    assert_eq!(
        score.ticks_per_quarter, reimported.ticks_per_quarter,
        "roundtrip must preserve PPQN"
    );
    assert_eq!(
        score.tracks.len(),
        reimported.tracks.len(),
        "roundtrip must preserve the note-bearing track count"
    );
});
