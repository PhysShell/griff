// TDD red phase for S6 rule-based generator.
// Fails to compile until the new S6 types and `generate()` are added to
// `griff_core::generate`.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::missing_assert_message,
    clippy::absolute_paths,
    clippy::missing_const_for_fn
)]

use griff_core::{
    event::{Event, Pitch, Tempo, Ticks, TimeSignature},
    generate::{
        generate, GenerationConstraints, GenerationError, GenerationSeed, GenerationStrategy,
        PitchMaterial, RuleGenerationRequest,
    },
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

fn quarter_rhythm() -> Vec<Ticks> {
    vec![Ticks(480); 4] // four quarter notes per bar
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

// ── determinism ───────────────────────────────────────────────────────────────

#[test]
fn same_seed_produces_identical_phrase() {
    let req = request(GenerationStrategy::ConstrainedRandomWalk);
    let a = generate(&req).expect("generate succeeds (a)");
    let b = generate(&req).expect("generate succeeds (b)");
    assert_eq!(
        a.phrase, b.phrase,
        "same seed must produce identical phrase"
    );
}

#[test]
fn different_seeds_produce_different_phrases() {
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
        a.phrase, b.phrase,
        "distinct seeds must produce distinct phrases"
    );
}

// ── structural invariants ─────────────────────────────────────────────────────

#[test]
fn output_bar_count_matches_request() {
    for strategy in all_strategies() {
        let req = request(strategy);
        let candidate = generate(&req).expect("generate succeeds");
        assert_eq!(
            candidate.phrase.bars.len(),
            req.constraints.bar_count,
            "{strategy:?}: bar count must match request",
        );
    }
}

#[test]
fn all_notes_within_pitch_range() {
    for strategy in all_strategies() {
        let req = request(strategy);
        let lo = req.constraints.pitch_lo.0;
        let hi = req.constraints.pitch_hi.0;
        let candidate = generate(&req).expect("generate succeeds");
        for bar in &candidate.phrase.bars {
            for event in &bar.events {
                if let Event::Note(n) = event {
                    assert!(
                        n.pitch.0 >= lo && n.pitch.0 <= hi,
                        "{strategy:?}: pitch {} is outside [{lo}, {hi}]",
                        n.pitch.0,
                    );
                }
            }
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
    assert_eq!(
        candidate.strategy, strategy,
        "candidate strategy must match request"
    );
    assert_eq!(candidate.seed, seed, "candidate seed must match request");
}

// ── per-strategy smoke tests ──────────────────────────────────────────────────

#[test]
fn rhythm_copy_produces_notes_with_source_durations() {
    let req = request(GenerationStrategy::RhythmCopyPitchSubstitute);
    let candidate = generate(&req).expect("generate succeeds");
    let note_durations: Vec<u32> = candidate
        .phrase
        .bars
        .iter()
        .flat_map(|b| b.events.iter())
        .filter_map(|e| match e {
            Event::Note(n) => Some(n.duration.0),
            Event::Rest(_) => None,
        })
        .collect();
    assert!(!note_durations.is_empty(), "rhythm copy must produce notes");
    for &dur in &note_durations {
        assert_eq!(
            dur, 480,
            "rhythm copy must use quarter-note durations from source"
        );
    }
}

#[test]
fn constrained_walk_limits_consecutive_pitch_leaps() {
    let req = request(GenerationStrategy::ConstrainedRandomWalk);
    let candidate = generate(&req).expect("generate succeeds");
    let pitches: Vec<u8> = candidate
        .phrase
        .bars
        .iter()
        .flat_map(|b| b.events.iter())
        .filter_map(|e| match e {
            Event::Note(n) => Some(n.pitch.0),
            Event::Rest(_) => None,
        })
        .collect();
    for pair in pitches.windows(2) {
        let leap = pair[0].abs_diff(pair[1]);
        assert!(
            leap <= 12,
            "constrained walk: leap {leap} semitones exceeds 12"
        );
    }
}

#[test]
fn motif_transpose_produces_at_least_one_note_per_bar() {
    let req = request(GenerationStrategy::MotifTransposeVariation);
    let candidate = generate(&req).expect("generate succeeds");
    for bar in &candidate.phrase.bars {
        let note_count = bar
            .events
            .iter()
            .filter(|e| matches!(e, Event::Note(_)))
            .count();
        assert!(
            note_count > 0,
            "motif transpose must put at least one note in every bar"
        );
    }
}

#[test]
fn shuffle_motifs_produces_notes() {
    let req = request(GenerationStrategy::ShuffleMotifs);
    let candidate = generate(&req).expect("generate succeeds");
    let total_notes: usize = candidate
        .phrase
        .bars
        .iter()
        .flat_map(|b| b.events.iter())
        .filter(|e| matches!(e, Event::Note(_)))
        .count();
    assert!(total_notes > 0, "shuffle motifs must produce notes");
}

#[test]
fn repeat_variation_second_bar_differs_from_first() {
    let req = request(GenerationStrategy::RepeatVariation);
    let candidate = generate(&req).expect("generate succeeds");
    assert_eq!(candidate.phrase.bars.len(), 2, "must produce 2 bars");
    assert_ne!(
        candidate.phrase.bars[0].events, candidate.phrase.bars[1].events,
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
    assert_eq!(generate(&req), Err(GenerationError::EmptyPitchMaterial));
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
    assert_eq!(generate(&req), Err(GenerationError::BarCountZero));
}

#[test]
fn rhythm_copy_without_source_rhythms_returns_error() {
    let req = RuleGenerationRequest {
        source_rhythms: Vec::new(),
        ..request(GenerationStrategy::RhythmCopyPitchSubstitute)
    };
    assert_eq!(generate(&req), Err(GenerationError::RhythmTemplateMissing));
}
