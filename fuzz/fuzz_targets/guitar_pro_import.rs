#![no_main]

//! Fuzz target: raw bytes → GP import → canonical Score (P0, ADR-0010).
//!
//! Flow:
//!   1. Try `import_gp_score(data)`.
//!   2. On success, run structural oracle checks on the returned [`Score`].
//!
//! Oracle (normalized invariants):
//!   * No panic / hang / unbounded allocation (libFuzzer limits).
//!   * `import_gp_score` returns `Ok(Score)` or a typed `GpImportError` —
//!     never an uncontrolled panic or abort.
//!   * Every `MasterBar.tick_range` satisfies `start <= end`.
//!   * `MasterBar` tick ranges are non-overlapping and in non-decreasing order.
//!   * Every `AtomEvent` duration is non-zero (guaranteed by `build_event_group`).
//!   * `Score.ticks_per_quarter` equals the GP PPQN constant (960).

use libfuzzer_sys::fuzz_target;

use griff_core::gp::import_gp_score;

fuzz_target!(|data: &[u8]| {
    let Ok(score) = import_gp_score(data) else {
        // Typed error (UnsupportedFormat or Parse) is a valid, expected outcome.
        return;
    };

    // ticks_per_quarter must equal GP PPQN.
    assert_eq!(
        score.ticks_per_quarter, 960,
        "ticks_per_quarter must be the GP PPQN (960)"
    );

    // Every master bar must have an ordered tick range.
    for mb in &score.master_bars {
        assert!(
            mb.tick_range.start.0 <= mb.tick_range.end.0,
            "MasterBar tick range must be ordered: [{}, {})",
            mb.tick_range.start.0,
            mb.tick_range.end.0,
        );
    }

    // Master bars must be in non-decreasing, non-overlapping order.
    let mut prev_end: u32 = 0;
    for mb in &score.master_bars {
        assert!(
            mb.tick_range.start.0 >= prev_end,
            "MasterBar tick ranges must be non-overlapping and in order"
        );
        prev_end = mb.tick_range.end.0;
    }

    // Every AtomEvent in every voice must have non-zero duration.
    for track in &score.tracks {
        for voice in &track.voices {
            for eg in &voice.event_groups {
                for atom in &eg.atoms {
                    assert!(
                        atom.duration().0 > 0,
                        "AtomEvent duration must be non-zero"
                    );
                }
            }
        }
    }
});
