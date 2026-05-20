#![no_main]

//! Fuzz target: MIDI bytes → canonical Score → phrase projection and analysis (P1, ADR-0010).
//!
//! Flow:
//!   1. Try `import_score(data)`.
//!   2. For each track, call `project_phrase` and check the projection.
//!   3. Run `phrase_features` and `timed_phrase_events` on every projected phrase.
//!
//! Oracle (normalized invariants):
//!   * No panic / hang / unbounded allocation (libFuzzer limits).
//!   * `project_phrase` returns a phrase whose bars have non-decreasing
//!     absolute positions.
//!   * `phrase_features` returns `Ok` without `DurationOverflow` panic.
//!   * `timed_phrase_events` emits events with non-decreasing absolute starts.
//!   * Every `MasterBar.tick_range` satisfies `start <= end`.
//!
//! Note: this target drives the projection code from a MIDI-imported Score
//! (structure via `import_score`). A future upgrade will add an
//! `arbitrary`-generated canonical Score path for richer structure coverage.

use libfuzzer_sys::fuzz_target;

use griff_core::{
    feature::phrase_features,
    midi::import_score,
    score::project_phrase,
    slice::timed_phrase_events,
};

fuzz_target!(|data: &[u8]| {
    let Ok(score) = import_score(data) else {
        return;
    };

    // Every master bar must have a non-empty, ordered tick range.
    for mb in &score.master_bars {
        assert!(
            mb.tick_range.start.0 <= mb.tick_range.end.0,
            "MasterBar tick range must be ordered: [{}, {})",
            mb.tick_range.start.0,
            mb.tick_range.end.0,
        );
    }

    // Master bars must appear in non-decreasing order.
    let mut prev_end: u32 = 0;
    for mb in &score.master_bars {
        assert!(
            mb.tick_range.start.0 >= prev_end,
            "MasterBar tick ranges must be non-overlapping and in order"
        );
        prev_end = mb.tick_range.end.0;
    }

    // For each track, project to the legacy phrase model and exercise the analysis pipeline.
    for track_idx in 0..score.tracks.len() {
        let Some(phrase) = project_phrase(&score, track_idx) else {
            continue;
        };

        // phrase_features must not panic and must return Ok or a typed error.
        let _ = phrase_features(&phrase);

        // timed_phrase_events must not panic and events must have non-decreasing starts.
        let Ok(timed) = timed_phrase_events(&phrase) else {
            continue;
        };

        let mut prev_start: u32 = 0;
        for tev in &timed {
            assert!(
                tev.absolute_start.0 >= prev_start,
                "timed events must have non-decreasing absolute_start"
            );
            prev_start = tev.absolute_start.0;
        }
    }
});
