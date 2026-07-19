#![no_main]

//! Fuzz oracle for `exact_semantic_diff` (S16 Phase 4-pre B1).
//!
//! Any score the MIDI importer accepts must satisfy the comparator's laws:
//!
//! * reflexivity — a score diffs empty against its own clone;
//! * determinism — the same diff computed twice is identical.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(score) = griff_core::midi::import_score(data) else {
        return;
    };
    let reflexive = griff_core::semantic_diff::exact_semantic_diff(&score, &score.clone());
    assert!(
        reflexive.is_empty(),
        "diff(s, s) must be empty, got {:#?}",
        reflexive.differences
    );

    let mut mutated = score.clone();
    mutated.ticks_per_quarter = mutated.ticks_per_quarter.wrapping_add(1).max(1);
    let first = griff_core::semantic_diff::exact_semantic_diff(&score, &mutated);
    let second = griff_core::semantic_diff::exact_semantic_diff(&score, &mutated);
    assert_eq!(first, second, "diffing must be deterministic");
});
