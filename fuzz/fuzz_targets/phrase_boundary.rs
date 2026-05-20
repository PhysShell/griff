#![no_main]

//! Fuzz target: MIDI bytes → canonical Score → phrase boundary detection (P1, ADR-0010).
//!
//! Flow:
//!   1. Try `import_score(data)`.
//!   2. For each track, call `detect_phrase_boundaries` with the default config.
//!   3. Assert oracle invariants on the returned boundaries.
//!
//! Oracle (normalized invariants):
//!   * No panic / hang / unbounded allocation (libFuzzer limits).
//!   * `detect_phrase_boundaries` returns `Ok(Vec)` — never panics.
//!   * Boundaries are sorted in non-decreasing order of `start_tick`.
//!   * Every boundary satisfies `start_tick <= end_tick`.
//!   * Every `boundary.score` is in `[0.0, 1.0]` and is finite (not NaN/inf).
//!   * `BoundaryReason::manual_override` is false for every non-manual boundary.

use libfuzzer_sys::fuzz_target;

use griff_core::{boundary::BoundaryConfig, boundary::detect_phrase_boundaries, midi::import_score};

fuzz_target!(|data: &[u8]| {
    let Ok(score) = import_score(data) else {
        return;
    };

    let config = BoundaryConfig::default();

    for track_idx in 0..score.tracks.len() {
        let boundaries = detect_phrase_boundaries(&score, track_idx, &config);

        // Boundaries must be sorted.
        let mut prev_start: u32 = 0;
        for b in &boundaries {
            assert!(
                b.start_tick.0 >= prev_start,
                "boundaries must be sorted by start_tick"
            );
            prev_start = b.start_tick.0;

            // start_tick <= end_tick.
            assert!(
                b.start_tick.0 <= b.end_tick.0,
                "boundary start_tick ({}) must be <= end_tick ({})",
                b.start_tick.0,
                b.end_tick.0,
            );

            // score must be finite and in [0.0, 1.0].
            assert!(
                b.score.is_finite(),
                "boundary score must be finite, got {}",
                b.score,
            );
            assert!(
                (0.0..=1.0).contains(&b.score),
                "boundary score must be in [0.0, 1.0], got {}",
                b.score,
            );

            // No non-manual boundary should carry manual_override = true.
            // (Manual overrides come only from config.manual_overrides, which is empty above.)
            assert!(
                !b.reason.manual_override,
                "default config has no overrides; manual_override must be false"
            );
        }
    }
});
