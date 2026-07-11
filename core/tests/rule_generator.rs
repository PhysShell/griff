// Characterization for the ADR-0011 step: `generate()` emits the canonical
// model — `GenerationCandidate` carries a `score: Score` rather than the
// retired legacy linear representation.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::missing_assert_message,
    clippy::absolute_paths,
    clippy::missing_const_for_fn
)]

use griff_core::{
    event::{Pitch, Tempo, Ticks, TimeSignature},
    generate::{
        generate, GenerationCandidate, GenerationConstraints, GenerationError, GenerationSeed,
        GenerationStrategy, PitchMaterial, RhythmTemplate, RuleGenerationRequest,
    },
    score::{AtomEvent, AtomNote, MasterBar, Voice},
};

// ── helpers ───────────────────────────────────────────────────────────────────

fn e_minor_pentatonic() -> PitchMaterial {
    PitchMaterial {
        root: Pitch(40), // E2
        intervals: vec![0, 3, 5, 7, 10],
    }
}

fn constraints_2_bars_4_4() -> GenerationConstraints {
    GenerationConstraints {
        bar_count: 2,
        time_signature: TimeSignature {
            numerator: 4,
            denominator: 4,
        },
        tempo: Tempo(120.0),
        ticks_per_quarter: Ticks(480),
        pitch_lo: Pitch(36), // C2
        pitch_hi: Pitch(72), // C5
    }
}

fn quarter_rhythm() -> RhythmTemplate {
    RhythmTemplate::from_durations(&[Ticks(480); 4]) // four quarter notes per bar
}

fn request(strategy: GenerationStrategy) -> RuleGenerationRequest {
    RuleGenerationRequest {
        seed: GenerationSeed(42),
        pitch_material: e_minor_pentatonic(),
        constraints: constraints_2_bars_4_4(),
        source_rhythms: vec![quarter_rhythm()],
        strategy,
    }
}

fn all_strategies() -> [GenerationStrategy; 5] {
    [
        GenerationStrategy::RhythmCopyPitchSubstitute,
        GenerationStrategy::MotifTransposeVariation,
        GenerationStrategy::ConstrainedRandomWalk,
        GenerationStrategy::ShuffleMotifs,
        GenerationStrategy::RepeatVariation,
    ]
}

/// The single generated voice (track 0, voice 0).
fn voice(candidate: &GenerationCandidate) -> &Voice {
    &candidate.score.tracks[0].voices[0]
}

/// All note atoms of a voice, in order.
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

/// Note pitches whose onset falls inside `mb`'s half-open tick range.
fn bar_pitches(ns: &[AtomNote], mb: &MasterBar) -> Vec<u8> {
    ns.iter()
        .filter(|n| {
            n.absolute_start.0 >= mb.tick_range.start.0 && n.absolute_start.0 < mb.tick_range.end.0
        })
        .map(|n| n.pitch.0)
        .collect()
}

// ── canonical output shape ──────────────────────────────────────────────────────

#[test]
fn output_is_a_single_track_single_voice_score() {
    let candidate =
        generate(&request(GenerationStrategy::ConstrainedRandomWalk)).expect("generate succeeds");
    assert_eq!(candidate.score.tracks.len(), 1, "one generated track");
    assert_eq!(
        candidate.score.tracks[0].voices.len(),
        1,
        "one voice in the generated track"
    );
}

#[test]
fn master_bar_count_matches_request() {
    for strategy in all_strategies() {
        let req = request(strategy);
        let candidate = generate(&req).expect("generate succeeds");
        assert_eq!(
            candidate.score.master_bars.len(),
            req.constraints.bar_count,
            "{strategy:?}: master-bar count must match request",
        );
    }
}

#[test]
fn score_carries_request_ppqn() {
    let candidate =
        generate(&request(GenerationStrategy::ShuffleMotifs)).expect("generate succeeds");
    assert_eq!(candidate.score.ticks_per_quarter, 480);
}

// ── determinism ───────────────────────────────────────────────────────────────

#[test]
fn same_seed_produces_identical_voice() {
    let req = request(GenerationStrategy::ConstrainedRandomWalk);
    let a = generate(&req).expect("generate succeeds (a)");
    let b = generate(&req).expect("generate succeeds (b)");
    assert_eq!(
        voice(&a),
        voice(&b),
        "same seed must produce identical voice"
    );
}

#[test]
fn different_seeds_produce_different_voices() {
    let req_a = RuleGenerationRequest {
        seed: GenerationSeed(1),
        ..request(GenerationStrategy::ConstrainedRandomWalk)
    };
    let req_b = RuleGenerationRequest {
        seed: GenerationSeed(99_999),
        ..request(GenerationStrategy::ConstrainedRandomWalk)
    };
    let a = generate(&req_a).expect("generate A");
    let b = generate(&req_b).expect("generate B");
    assert_ne!(
        voice(&a),
        voice(&b),
        "distinct seeds must produce distinct voices"
    );
}

// ── structural invariants ─────────────────────────────────────────────────────

#[test]
fn all_notes_within_pitch_range() {
    for strategy in all_strategies() {
        let req = request(strategy);
        let lo = req.constraints.pitch_lo.0;
        let hi = req.constraints.pitch_hi.0;
        let candidate = generate(&req).expect("generate succeeds");
        for n in notes(voice(&candidate)) {
            assert!(
                n.pitch.0 >= lo && n.pitch.0 <= hi,
                "{strategy:?}: pitch {} is outside [{lo}, {hi}]",
                n.pitch.0,
            );
        }
    }
}

#[test]
fn note_onsets_are_non_decreasing() {
    for strategy in all_strategies() {
        let candidate = generate(&request(strategy)).expect("generate succeeds");
        let onsets: Vec<u32> = notes(voice(&candidate))
            .iter()
            .map(|n| n.absolute_start.0)
            .collect();
        for pair in onsets.windows(2) {
            assert!(
                pair[0] <= pair[1],
                "{strategy:?}: onsets must be non-decreasing ({} then {})",
                pair[0],
                pair[1],
            );
        }
    }
}

#[test]
fn candidate_carries_strategy_and_seed() {
    let strategy = GenerationStrategy::RepeatVariation;
    let seed = GenerationSeed(12_345);
    let req = RuleGenerationRequest {
        seed,
        ..request(strategy)
    };
    let candidate = generate(&req).expect("generate succeeds");
    assert_eq!(candidate.strategy, strategy, "strategy must match request");
    assert_eq!(candidate.seed, seed, "seed must match request");
}

// ── per-strategy smoke tests ──────────────────────────────────────────────────

#[test]
fn rhythm_copy_produces_notes_with_source_durations() {
    let candidate = generate(&request(GenerationStrategy::RhythmCopyPitchSubstitute))
        .expect("generate succeeds");
    let durations: Vec<u32> = notes(voice(&candidate))
        .iter()
        .map(|n| n.duration.0)
        .collect();
    assert!(!durations.is_empty(), "rhythm copy must produce notes");
    for &dur in &durations {
        assert_eq!(
            dur, 480,
            "rhythm copy must use quarter-note durations from source"
        );
    }
}

#[test]
fn constrained_walk_limits_consecutive_pitch_leaps() {
    let candidate =
        generate(&request(GenerationStrategy::ConstrainedRandomWalk)).expect("generate succeeds");
    let pitches: Vec<u8> = notes(voice(&candidate)).iter().map(|n| n.pitch.0).collect();
    for pair in pitches.windows(2) {
        let leap = pair[0].abs_diff(pair[1]);
        assert!(
            leap <= 12,
            "constrained walk: leap {leap} semitones exceeds 12"
        );
    }
}

#[test]
fn motif_transpose_puts_a_note_in_every_bar() {
    let candidate =
        generate(&request(GenerationStrategy::MotifTransposeVariation)).expect("generate succeeds");
    let ns = notes(voice(&candidate));
    for mb in &candidate.score.master_bars {
        assert!(
            !bar_pitches(&ns, mb).is_empty(),
            "motif transpose must put at least one note in every bar"
        );
    }
}

#[test]
fn shuffle_motifs_produces_notes() {
    let candidate =
        generate(&request(GenerationStrategy::ShuffleMotifs)).expect("generate succeeds");
    assert!(
        !notes(voice(&candidate)).is_empty(),
        "shuffle motifs must produce notes"
    );
}

#[test]
fn repeat_variation_second_bar_differs_from_first() {
    let candidate =
        generate(&request(GenerationStrategy::RepeatVariation)).expect("generate succeeds");
    assert_eq!(candidate.score.master_bars.len(), 2, "must produce 2 bars");
    let ns = notes(voice(&candidate));
    assert_ne!(
        bar_pitches(&ns, &candidate.score.master_bars[0]),
        bar_pitches(&ns, &candidate.score.master_bars[1]),
        "repeat variation: bar 1 must differ from bar 0",
    );
}

// ── error cases ───────────────────────────────────────────────────────────────

#[test]
fn empty_pitch_material_returns_error() {
    let req = RuleGenerationRequest {
        pitch_material: PitchMaterial {
            root: Pitch(40),
            intervals: Vec::new(),
        },
        ..request(GenerationStrategy::ConstrainedRandomWalk)
    };
    assert!(matches!(
        generate(&req),
        Err(GenerationError::EmptyPitchMaterial)
    ));
}

#[test]
fn zero_bar_count_returns_error() {
    let req = RuleGenerationRequest {
        constraints: GenerationConstraints {
            bar_count: 0,
            ..constraints_2_bars_4_4()
        },
        ..request(GenerationStrategy::ConstrainedRandomWalk)
    };
    assert!(matches!(generate(&req), Err(GenerationError::BarCountZero)));
}

#[test]
fn rhythm_copy_without_source_rhythms_returns_error() {
    let req = RuleGenerationRequest {
        source_rhythms: Vec::new(),
        ..request(GenerationStrategy::RhythmCopyPitchSubstitute)
    };
    assert!(matches!(
        generate(&req),
        Err(GenerationError::RhythmTemplateMissing)
    ));
}

#[test]
fn timeline_overflow_returns_error() {
    // Each ingredient is individually valid (PPQN fits u16, meter is legal),
    // but bar_duration * bar_count overflows the u32 master timeline. The
    // canonical MasterBar ranges must not silently truncate; reject instead.
    let req = RuleGenerationRequest {
        constraints: GenerationConstraints {
            bar_count: 100_000,
            ticks_per_quarter: Ticks(65_535),
            ..constraints_2_bars_4_4()
        },
        ..request(GenerationStrategy::ConstrainedRandomWalk)
    };
    assert!(matches!(
        generate(&req),
        Err(GenerationError::InvalidConstraints)
    ));
}

#[test]
fn ppqn_above_u16_returns_error() {
    // Score::ticks_per_quarter is u16; a request whose PPQN exceeds u16::MAX
    // cannot be represented faithfully. Reject rather than clamp (which would
    // make onsets, computed at the real PPQN, inconsistent with the score).
    let req = RuleGenerationRequest {
        constraints: GenerationConstraints {
            ticks_per_quarter: Ticks(70_000),
            ..constraints_2_bars_4_4()
        },
        ..request(GenerationStrategy::ConstrainedRandomWalk)
    };
    assert!(matches!(
        generate(&req),
        Err(GenerationError::InvalidConstraints)
    ));
}
