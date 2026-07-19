#![no_main]

//! Fuzz oracle for `exact_semantic_diff` and `normalized_musical_diff`
//! (S16 Phase 4-pre B1/B2).
//!
//! Any score the MIDI importer accepts must satisfy both comparators' laws:
//!
//! * reflexivity — a score diffs empty against its own clone, under the
//!   exact contract and under the v1 normalized-musical policy;
//! * determinism — the same diff computed twice is identical, including
//!   after a safe PPQN mutation.

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

    let normalized_reflexive =
        griff_core::semantic_diff::normalized_musical_diff(&score, &score.clone());
    assert!(
        normalized_reflexive.is_empty(),
        "normalized diff(s, s) must be empty, got {:#?}",
        normalized_reflexive.differences
    );

    let mut mutated = score.clone();
    mutated.ticks_per_quarter = mutated.ticks_per_quarter.wrapping_add(1).max(1);
    let first = griff_core::semantic_diff::exact_semantic_diff(&score, &mutated);
    let second = griff_core::semantic_diff::exact_semantic_diff(&score, &mutated);
    assert_eq!(first, second, "exact diffing must be deterministic");

    let norm_first = griff_core::semantic_diff::normalized_musical_diff(&score, &mutated);
    let norm_second = griff_core::semantic_diff::normalized_musical_diff(&score, &mutated);
    assert_eq!(
        norm_first, norm_second,
        "normalized diffing must be deterministic"
    );
});
