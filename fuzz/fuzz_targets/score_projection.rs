#![no_main]

//! Fuzz target: MIDI bytes → canonical Score → voice analysis (P1, ADR-0010).
//!
//! Flow:
//!   1. Try `import_score(data)`.
//!   2. Check master-bar timeline invariants.
//!   3. For each track/voice, run `voice_features` and check atom ordering.
//!
//! Oracle (normalized invariants):
//!   * No panic / hang / unbounded allocation (libFuzzer limits).
//!   * Every `MasterBar.tick_range` satisfies `start <= end`, and master bars
//!     appear in non-decreasing, non-overlapping order.
//!   * `voice_features` returns `Ok` without a `DurationOverflow` panic.
//!   * Atom events within a voice have non-decreasing absolute starts.
//!
//! Note: this target drives the canonical analysis pipeline from a
//! MIDI-imported Score (structure via `import_score`). A future upgrade will add
//! an `arbitrary`-generated canonical Score path for richer structure coverage.

use libfuzzer_sys::fuzz_target;

use griff_core::{feature::voice_features, midi::import_score, score::AtomEvent};

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

    // For each track/voice, exercise the canonical analysis pipeline.
    for track in &score.tracks {
        for voice in &track.voices {
            // voice_features must not panic and must return Ok or a typed error.
            let _ = voice_features(voice);

            // Atom events must have non-decreasing absolute starts.
            let mut prev_start: u32 = 0;
            for group in &voice.event_groups {
                for atom in &group.atoms {
                    let start = match atom {
                        AtomEvent::Note(n) => n.absolute_start.0,
                        AtomEvent::Rest(r) => r.absolute_start.0,
                    };
                    assert!(
                        start >= prev_start,
                        "atom events must have non-decreasing absolute_start"
                    );
                    prev_start = start;
                }
            }
        }
    }
});
