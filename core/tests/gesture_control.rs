// TDD red phase: gesture control — the corpus gesture statistics become S6
// constraint inputs (melodic-closure note §3.5/§7.4; decisions.log
// 2026-06-11: the stats are "corpus facts and future S6 constraint inputs").
//
// The shape is the established constraint-compiler-over-S6 pattern
// (ComplementArranger ADR-0012, StructureControl ADR-0015), not a new
// generation core: `GestureControl` is the control counterpart of the
// measured `GestureStats` (what you *ask for* vs what it *is*), derivable
// from a corpus chunk via `GestureControl::from_stats`. `generate_gestured`
// runs the plain S6 generator, then deterministically carves gesture rests —
// after every `burst_notes` kept notes it drops following notes until at
// least `rest_quarters` of line silence opens up — and returns the produced
// score with its re-measured `GestureStats` as provenance.
//
// S6 strategies write wall-to-wall (every bar filled back-to-back), so
// carving is exactly what turns S6 output into burst-and-rest writing; the
// surviving notes keep their absolute onsets (a carve never moves a note).
//
// References `griff_core::gesture::{GestureControl, generate_gestured, …}`,
// which do not exist yet, so the suite fails until the green step.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::missing_assert_message,
    clippy::float_cmp,
    clippy::arithmetic_side_effects
)]

use griff_core::{
    event::{Pitch, Tempo, Ticks, TimeSignature},
    generate::{
        generate, GenerationConstraints, GenerationError, GenerationSeed, GenerationStrategy,
        PitchMaterial, RuleGenerationRequest,
    },
    gesture::{generate_gestured, measure_gesture, GestureControl, GestureGenError, GestureStats},
    score::{AtomEvent, AtomNote, Score, Voice},
};

// ── helpers ───────────────────────────────────────────────────────────────────

/// Gesture stats whose only meaningful fields here are the two the control
/// derives from; the rest are plausible fillers.
fn stats(mean_burst_notes: f64, mean_rest_quarters: f64) -> GestureStats {
    GestureStats {
        note_count: 12,
        burst_count: 3,
        mean_burst_notes,
        max_burst_notes: 6,
        rest_count: if mean_rest_quarters == 0.0 { 0 } else { 2 },
        mean_rest_quarters,
        rest_on_grid_share: 1.0,
        modal_landing_share: 0.5,
        mean_final_lengthening: 0.5,
    }
}

/// A 4-bar 4/4 `ConstrainedRandomWalk` request at 480 PPQN: S6 fills it
/// wall-to-wall with sixteen quarter notes (four per bar, onsets `i × 480`).
fn walk_request() -> RuleGenerationRequest {
    RuleGenerationRequest {
        seed: GenerationSeed(42),
        pitch_material: PitchMaterial {
            root: Pitch(40), // E2
            intervals: vec![0, 3, 5, 7, 10],
        },
        constraints: GenerationConstraints {
            bar_count: 4,
            time_signature: TimeSignature {
                numerator: 4,
                denominator: 4,
            },
            tempo: Tempo::from_bpm_integer(120).expect("valid BPM"),
            ticks_per_quarter: Ticks(480),
            pitch_lo: Pitch(36), // C2
            pitch_hi: Pitch(72), // C5
        },
        explicit_rhythms: None,
        source_rhythms: Vec::new(),
        strategy: GenerationStrategy::ConstrainedRandomWalk,
    }
}

/// The single generated voice (track 0, voice 0).
fn voice(score: &Score) -> &Voice {
    &score.tracks[0].voices[0]
}

/// All note atoms of a voice, in onset order.
fn notes(voice: &Voice) -> Vec<AtomNote> {
    voice
        .event_groups
        .iter()
        .flat_map(|g| g.atoms.iter())
        .filter_map(|a| match a {
            AtomEvent::Note(n) => Some(*n),
            AtomEvent::Rest(_) => None,
        })
        .collect()
}

// ── deriving the control from corpus stats ────────────────────────────────────

#[test]
fn from_stats_rounds_burst_and_keeps_rest() {
    let control = GestureControl::from_stats(&stats(5.25, 2.5));
    assert_eq!(control.burst_notes, 5, "mean burst rounds to nearest");
    assert_eq!(control.rest_quarters, 2.5, "rest length carries over");
}

#[test]
fn from_stats_clamps_to_the_gesture_floor() {
    // A gesture rest is at least one quarter by definition (sub-quarter holes
    // are phrasing), and a burst has at least one note — a restless chunk
    // (mean 0.0) derives the minimal gesture, not a degenerate one.
    let control = GestureControl::from_stats(&stats(0.4, 0.0));
    assert_eq!(control.burst_notes, 1);
    assert_eq!(control.rest_quarters, 1.0);
}

// ── control validation ────────────────────────────────────────────────────────

#[test]
fn invalid_controls_are_typed_errors() {
    let zero_burst = GestureControl {
        burst_notes: 0,
        rest_quarters: 1.0,
    };
    assert!(matches!(
        generate_gestured(&walk_request(), zero_burst),
        Err(GestureGenError::InvalidControl)
    ));

    let sub_quarter_rest = GestureControl {
        burst_notes: 4,
        rest_quarters: 0.5,
    };
    assert!(matches!(
        generate_gestured(&walk_request(), sub_quarter_rest),
        Err(GestureGenError::InvalidControl)
    ));

    let non_finite_rest = GestureControl {
        burst_notes: 4,
        rest_quarters: f64::NAN,
    };
    assert!(matches!(
        generate_gestured(&walk_request(), non_finite_rest),
        Err(GestureGenError::InvalidControl)
    ));
}

#[test]
fn generation_errors_pass_through() {
    let request = RuleGenerationRequest {
        pitch_material: PitchMaterial {
            root: Pitch(40),
            intervals: Vec::new(),
        },
        ..walk_request()
    };
    let control = GestureControl {
        burst_notes: 3,
        rest_quarters: 1.0,
    };
    assert!(matches!(
        generate_gestured(&request, control),
        Err(GestureGenError::Generation(
            GenerationError::EmptyPitchMaterial
        ))
    ));
}

// ── carving ───────────────────────────────────────────────────────────────────

#[test]
fn carves_gesture_rests_to_the_control() {
    // Sixteen quarter notes, burst 3 / rest 1: keep 3, carve 1, repeated —
    // twelve survivors in four 3-note bursts, four one-quarter rests (the
    // carved trailing quarter included), all on the quarter grid.
    let request = walk_request();
    let control = GestureControl {
        burst_notes: 3,
        rest_quarters: 1.0,
    };
    let candidate = generate_gestured(&request, control).expect("gestured generation succeeds");

    assert_eq!(candidate.seed, request.seed, "seed is provenance");
    assert_eq!(candidate.control, control, "control is provenance");
    assert_eq!(
        candidate.score.master_bars.len(),
        request.constraints.bar_count,
        "carving must not touch the master timeline"
    );

    let stats = candidate.stats;
    assert_eq!(stats.note_count, 12);
    assert_eq!(stats.burst_count, 4);
    assert_eq!(stats.mean_burst_notes, 3.0);
    assert_eq!(stats.max_burst_notes, 3);
    assert_eq!(stats.rest_count, 4);
    assert_eq!(stats.mean_rest_quarters, 1.0);
    assert_eq!(
        stats.rest_on_grid_share, 1.0,
        "quarter-pace carving lands rests on the quarter grid"
    );
    assert_eq!(
        stats.mean_final_lengthening, 0.5,
        "equal durations: every landing reads equal-to-mean"
    );
}

#[test]
fn provenance_stats_match_a_remeasure() {
    let candidate = generate_gestured(
        &walk_request(),
        GestureControl {
            burst_notes: 3,
            rest_quarters: 1.0,
        },
    )
    .expect("gestured generation succeeds");
    let remeasured = measure_gesture(&candidate.score, 0).expect("output is measurable");
    assert_eq!(
        candidate.stats, remeasured,
        "the carried stats are the measured output, not the ask"
    );
}

#[test]
fn longer_rest_targets_carve_longer_rests() {
    // Burst 3 / rest 2: keep 3, carve 2 — survivors 3+3+3+1, three two-quarter
    // rests, and a short final burst (the carve discipline does not pad).
    let candidate = generate_gestured(
        &walk_request(),
        GestureControl {
            burst_notes: 3,
            rest_quarters: 2.0,
        },
    )
    .expect("gestured generation succeeds");

    let stats = candidate.stats;
    assert_eq!(stats.note_count, 10);
    assert_eq!(stats.burst_count, 4);
    assert_eq!(stats.mean_burst_notes, 2.5);
    assert_eq!(stats.max_burst_notes, 3);
    assert_eq!(stats.rest_count, 3);
    assert_eq!(stats.mean_rest_quarters, 2.0);
}

#[test]
fn bursts_span_bar_boundaries() {
    // Burst 5 at quarter pace is longer than a 4/4 bar, so a kept burst must
    // cross the barline — bursts are runs between gesture rests, not bars.
    let candidate = generate_gestured(
        &walk_request(),
        GestureControl {
            burst_notes: 5,
            rest_quarters: 1.0,
        },
    )
    .expect("gestured generation succeeds");

    let stats = candidate.stats;
    assert_eq!(stats.note_count, 14, "5 + 5 + 4 survivors");
    assert_eq!(stats.burst_count, 3);
    assert_eq!(stats.max_burst_notes, 5);
    assert_eq!(stats.rest_count, 2);
    assert_eq!(stats.mean_rest_quarters, 1.0);
}

#[test]
fn carving_only_removes_notes() {
    // Every surviving note is one of the plain S6 notes, unmoved: same onset,
    // same pitch, same duration — a carve opens silence, it never rewrites.
    let request = walk_request();
    let plain = generate(&request).expect("plain S6 succeeds");
    let gestured = generate_gestured(
        &request,
        GestureControl {
            burst_notes: 3,
            rest_quarters: 1.0,
        },
    )
    .expect("gestured generation succeeds");

    let plain_notes = notes(voice(&plain.score));
    let kept = notes(voice(&gestured.score));
    assert!(kept.len() < plain_notes.len(), "something was carved");
    for note in &kept {
        assert!(
            plain_notes.contains(note),
            "kept note at tick {} must be a plain S6 note",
            note.absolute_start.0
        );
    }
}

#[test]
fn burst_target_above_note_count_keeps_everything() {
    let request = walk_request();
    let plain = generate(&request).expect("plain S6 succeeds");
    let gestured = generate_gestured(
        &request,
        GestureControl {
            burst_notes: 100,
            rest_quarters: 1.0,
        },
    )
    .expect("gestured generation succeeds");

    assert_eq!(
        voice(&gestured.score),
        voice(&plain.score),
        "nothing to carve: the gestured voice is the plain S6 voice"
    );
    assert_eq!(gestured.stats.burst_count, 1);
    assert_eq!(gestured.stats.rest_count, 0);
}

#[test]
fn gestured_generation_is_deterministic() {
    let request = walk_request();
    let control = GestureControl {
        burst_notes: 3,
        rest_quarters: 1.0,
    };
    let a = generate_gestured(&request, control).expect("gestured generation succeeds (a)");
    let b = generate_gestured(&request, control).expect("gestured generation succeeds (b)");
    assert_eq!(
        voice(&a.score),
        voice(&b.score),
        "same request and control must produce the identical voice"
    );
    assert_eq!(a.stats, b.stats);
}
