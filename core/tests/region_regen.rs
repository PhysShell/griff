// TDD red phase for S11 region regeneration.
// Fails to compile until `griff_core::regen` is implemented.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::missing_assert_message,
    clippy::absolute_paths,
    clippy::missing_const_for_fn
)]

use griff_core::{
    event::{Bar, Event, Note, Phrase, Pitch, Tempo, Ticks, TimeSignature, Velocity},
    generate::{GenerationConstraints, GenerationSeed, GenerationStrategy, PitchMaterial},
    regen::{
        regenerate, AnchorPoint, ContinuityCheck, FrozenRegion, RegenerationConstraints,
        RegenerationError, RegenerationRegion, RegenerationRequest,
    },
    slice::TickRange,
};

// ── helpers ───────────────────────────────────────────────────────────────────

/// One-note bar: a single note filling the whole 4/4 bar at 480 tpq (1920 ticks).
fn note_bar(pitch: u8) -> Bar {
    Bar {
        time_signature: TimeSignature {
            numerator: 4,
            denominator: 4,
        },
        tempo: Tempo(120.0),
        events: vec![Event::Note(Note {
            pitch: Pitch(pitch),
            duration: Ticks(1920),
            velocity: Velocity(80),
            articulation: None,
        })],
    }
}

/// 4-bar phrase.  Bar tick positions: 0, 1920, 3840, 5760; end = 7680.
fn four_bar_phrase() -> Phrase {
    Phrase {
        bars: vec![
            note_bar(60), // bar 0  ticks  0..1920   C4
            note_bar(62), // bar 1  ticks  1920..3840 D4
            note_bar(64), // bar 2  ticks  3840..5760 E4
            note_bar(65), // bar 3  ticks  5760..7680 F4
        ],
    }
}

/// Generation constraints locked to a single pitch (60) so outputs are
/// deterministically verifiable regardless of strategy.
fn locked_constraints(bar_count: usize) -> GenerationConstraints {
    GenerationConstraints {
        bar_count,
        time_signature: TimeSignature {
            numerator: 4,
            denominator: 4,
        },
        tempo: Tempo(120.0),
        ticks_per_quarter: Ticks(480),
        pitch_lo: Pitch(60),
        pitch_hi: Pitch(60),
    }
}

fn locked_pitch_material() -> PitchMaterial {
    PitchMaterial {
        root: Pitch(60),
        intervals: vec![0],
    }
}

/// Region covering bars 1–2 (ticks 1920..5760) of the four-bar phrase.
fn region_bars_1_2() -> RegenerationRegion {
    RegenerationRegion {
        range: TickRange {
            start: Ticks(1920),
            end: Ticks(5760),
        },
    }
}

/// Minimal request regenerating bars 1–2 freely with no frozen regions.
fn free_request() -> RegenerationRequest {
    RegenerationRequest {
        source: four_bar_phrase(),
        region: region_bars_1_2(),
        constraints: RegenerationConstraints {
            frozen: Vec::new(),
            anchors: Vec::new(),
        },
        pitch_material: locked_pitch_material(),
        source_rhythms: Vec::new(),
        generation_constraints: locked_constraints(2),
        strategy: GenerationStrategy::ConstrainedRandomWalk,
        seed: GenerationSeed(42),
    }
}

// ── structural invariants ─────────────────────────────────────────────────────

#[test]
fn output_has_same_bar_count_as_source() {
    let req = free_request();
    let out = regenerate(&req).expect("regenerate succeeds");
    assert_eq!(
        out.phrase.bars.len(),
        req.source.bars.len(),
        "output bar count must match source"
    );
}

#[test]
fn bars_before_region_unchanged() {
    let req = free_request();
    let out = regenerate(&req).expect("regenerate succeeds");
    assert_eq!(
        out.phrase.bars[0], req.source.bars[0],
        "bar 0 (before region) must be unchanged"
    );
}

#[test]
fn bars_after_region_unchanged() {
    let req = free_request();
    let out = regenerate(&req).expect("regenerate succeeds");
    assert_eq!(
        out.phrase.bars[3], req.source.bars[3],
        "bar 3 (after region) must be unchanged"
    );
}

// ── frozen regions ────────────────────────────────────────────────────────────

#[test]
fn frozen_bar_is_byte_stable() {
    let req = RegenerationRequest {
        constraints: RegenerationConstraints {
            frozen: vec![FrozenRegion {
                range: TickRange {
                    start: Ticks(1920),
                    end: Ticks(3840),
                },
            }],
            anchors: Vec::new(),
        },
        ..free_request()
    };
    let out = regenerate(&req).expect("regenerate succeeds");
    assert_eq!(
        out.phrase.bars[1], req.source.bars[1],
        "bar 1 (frozen) must be byte-stable"
    );
}

#[test]
fn non_frozen_bar_in_region_is_replaced() {
    // Source bar 1 has pitch 62; with pitch locked to 60, the regenerated bar
    // must differ (all notes at 60, not 62).
    let req = free_request();
    let out = regenerate(&req).expect("regenerate succeeds");
    let source_pitch = match req.source.bars[1].events[0] {
        Event::Note(n) => n.pitch.0,
        Event::Rest(_) => 0,
    };
    let regen_pitch = out.phrase.bars[1]
        .events
        .iter()
        .filter_map(|e| match e {
            Event::Note(n) => Some(n.pitch.0),
            Event::Rest(_) => None,
        })
        .next();
    if let Some(p) = regen_pitch {
        assert_ne!(p, source_pitch, "regenerated bar must differ from source");
    }
}

#[test]
fn all_frozen_means_output_equals_source_in_region() {
    let req = RegenerationRequest {
        constraints: RegenerationConstraints {
            frozen: vec![
                FrozenRegion {
                    range: TickRange {
                        start: Ticks(1920),
                        end: Ticks(3840),
                    },
                },
                FrozenRegion {
                    range: TickRange {
                        start: Ticks(3840),
                        end: Ticks(5760),
                    },
                },
            ],
            anchors: Vec::new(),
        },
        ..free_request()
    };
    let out = regenerate(&req).expect("regenerate succeeds");
    assert_eq!(
        out.phrase.bars[1], req.source.bars[1],
        "bar 1 fully frozen must be unchanged"
    );
    assert_eq!(
        out.phrase.bars[2], req.source.bars[2],
        "bar 2 fully frozen must be unchanged"
    );
}

// ── determinism ───────────────────────────────────────────────────────────────

#[test]
fn same_seed_produces_identical_output() {
    let req = free_request();
    let out_a = regenerate(&req).expect("regenerate a");
    let out_b = regenerate(&req).expect("regenerate b");
    assert_eq!(
        out_a.phrase, out_b.phrase,
        "same seed must produce identical output"
    );
}

#[test]
fn different_seeds_produce_different_output() {
    let req_a = free_request();
    let req_b = RegenerationRequest {
        seed: GenerationSeed(99_999),
        ..free_request()
    };
    let out_a = regenerate(&req_a).expect("regenerate a");
    let out_b = regenerate(&req_b).expect("regenerate b");
    assert_ne!(
        out_a.phrase.bars[1], out_b.phrase.bars[1],
        "different seeds must produce different regenerated bars"
    );
}

// ── continuity ────────────────────────────────────────────────────────────────

#[test]
fn left_join_passes_for_small_interval() {
    // Source bar 0 has pitch 60; generator locked to pitch 60.
    // Interval = |60 - 60| = 0 ≤ 7 → left join passes.
    let req = free_request();
    let out = regenerate(&req).expect("regenerate succeeds");
    assert!(
        out.continuity.left_join_passes,
        "left join must pass when interval is 0"
    );
}

#[test]
fn right_join_fails_for_large_leap() {
    // Source bar 3 has pitch 65 (F4).  Generator locked to 60 (C4).
    // Interval = |60 - 65| = 5 ≤ 7 → actually passes.
    // Use pitch 72 (C5) in bar 3 instead: |60 - 72| = 12 > 7 → fails.
    let mut source = four_bar_phrase();
    source.bars[3] = note_bar(72); // C5 after the region
    let req = RegenerationRequest {
        source,
        ..free_request()
    };
    let out = regenerate(&req).expect("regenerate succeeds");
    assert!(
        !out.continuity.right_join_passes,
        "right join must fail when leap is 12 semitones (> 7)"
    );
}

#[test]
fn right_join_passes_for_small_interval() {
    // Bar 3 has pitch 60; generator locked to 60; interval = 0.
    let mut source = four_bar_phrase();
    source.bars[3] = note_bar(60);
    let req = RegenerationRequest {
        source,
        ..free_request()
    };
    let out = regenerate(&req).expect("regenerate succeeds");
    assert!(
        out.continuity.right_join_passes,
        "right join must pass when interval is 0"
    );
}

#[test]
fn continuity_check_exposes_interval_values() {
    let req = free_request();
    let out = regenerate(&req).expect("regenerate succeeds");
    // left_interval: |60(bar0) - 60(generated)| = 0
    // We just verify the field type is accessible and contains a value or None.
    let _: ContinuityCheck = out.continuity;
}

// ── error cases ───────────────────────────────────────────────────────────────

#[test]
fn region_end_beyond_phrase_returns_error() {
    let req = RegenerationRequest {
        region: RegenerationRegion {
            range: TickRange {
                start: Ticks(5760),
                end: Ticks(9999), // 9999 > 7680
            },
        },
        generation_constraints: locked_constraints(2),
        ..free_request()
    };
    let result = regenerate(&req);
    assert!(
        matches!(result, Err(RegenerationError::RegionOutOfBounds { .. })),
        "region end beyond phrase must return RegionOutOfBounds error"
    );
}

// ── anchor points (structural, no semantic check yet) ─────────────────────────

#[test]
fn request_with_anchor_points_succeeds() {
    let req = RegenerationRequest {
        constraints: RegenerationConstraints {
            frozen: Vec::new(),
            anchors: vec![AnchorPoint {
                tick: Ticks(1920),
                pitch: Pitch(60),
            }],
        },
        ..free_request()
    };
    // Anchor points are recorded but don't yet hard-constrain generation.
    // This test verifies the API compiles and the function runs without panic.
    let result = regenerate(&req);
    assert!(result.is_ok(), "request with anchor points must succeed");
}
