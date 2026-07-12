// TDD red phase: the full-range scale ladder — the register/pitch increment
// (arbiter 2026-07-12). The rotation A/B exposed a narrow, low register (~one
// octave, root E1): `rhythm_copy`/`shuffle_motifs` walked scale degrees only
// in `[0, scale_len)` — one octave up from `PitchMaterial.root` (the input's
// MINIMUM pitch) — and `motif_transpose` shifted by *semitones*, which can
// leave the pitch-class palette entirely.
//
// The fix is a single shared degree->pitch mapper: a `ScaleLadder` over a
// `PitchRange` and a `PitchClassSet`, reused by the generator and (via
// `band_scale_ladder`) the complement arranger — no second mapper. The
// contract this suite pins:
//
// - pitches ALWAYS fall inside `[pitch_lo, pitch_hi]`;
// - generated pitches ALWAYS belong to the allowed pitch classes;
// - the candidate set is no longer structurally confined to the first octave
//   (a full in-range ladder is REACHABLE — not that every single piece must
//   visit the whole range);
// - determinism under a fixed request holds;
// - a narrow range still works; reversed/equal bounds do not regress.
//
// References `griff_core::pitch::{PitchRange, PitchClassSet, ScaleLadder}`,
// which do not exist yet, so the suite fails to compile until the green step.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::missing_assert_message,
    clippy::missing_const_for_fn,
    clippy::arithmetic_side_effects
)]

use std::collections::BTreeSet;

use griff_core::{
    event::{Pitch, Tempo, Ticks, TimeSignature},
    generate::{
        generate, GenerationConstraints, GenerationSeed, GenerationStrategy, PitchMaterial,
        RuleGenerationRequest,
    },
    pitch::{PitchClassSet, PitchRange, ScaleLadder},
    rerank::{generate_candidate_set, SetRequest},
    score::{AtomEvent, Score},
};

// ── shared primitive ──────────────────────────────────────────────────────────

#[test]
fn pitch_class_set_normalises_and_dedups() {
    let set = PitchClassSet::new([4, 16, 7, 7, 24]); // 16%12=4, 24%12=0, dup 7
    assert_eq!(set.classes(), &[0, 4, 7], "sorted, mod 12, deduped");
    assert!(set.contains_pitch(Pitch(40))); // 40%12 = 4
    assert!(!set.contains_pitch(Pitch(41))); // 41%12 = 5
}

#[test]
fn pitch_range_normalises_reversed_and_clamped_bounds() {
    // Reversed bounds must not panic or invert; MIDI clamps at 127.
    let r = PitchRange::new(Pitch(72), Pitch(36));
    assert_eq!((r.lo.0, r.hi.0), (36, 72));
    let equal = PitchRange::new(Pitch(50), Pitch(50));
    assert_eq!((equal.lo.0, equal.hi.0), (50, 50));
}

#[test]
fn scale_ladder_spans_the_full_range_in_class() {
    // E minor pentatonic classes {E,G,A,B,D} over E1..E4 (28..=64): every rung
    // is in class, ascending, and the ladder covers more than one octave.
    let classes = PitchClassSet::new([2, 4, 7, 9, 11]); // D E G A B
    let ladder = ScaleLadder::build(&PitchRange::new(Pitch(28), Pitch(64)), &classes);

    assert!(ladder.len() > 5, "a 3-octave pentatonic ladder is long");
    let pitches: Vec<u8> = ladder.pitches().iter().map(|p| p.0).collect();
    for w in pitches.windows(2) {
        assert!(w[0] < w[1], "ladder is strictly ascending");
    }
    for p in &pitches {
        assert!((28..=64).contains(p), "rung {p} in range");
        assert!(classes.contains_pitch(Pitch(*p)), "rung {p} in class");
    }
    assert!(
        pitches.last().unwrap() - pitches.first().unwrap() > 12,
        "ladder spans more than one octave"
    );
}

#[test]
fn scale_ladder_empty_class_set_falls_back_to_lo() {
    let ladder = ScaleLadder::build(&PitchRange::new(Pitch(40), Pitch(60)), &PitchClassSet::new([]));
    assert_eq!(ladder.len(), 1);
    assert_eq!(ladder.pitches(), &[Pitch(40)]);
}

// ── generation contract ───────────────────────────────────────────────────────

/// E minor pentatonic anchored at E2 (root 40): intervals 0,3,5,7,10 →
/// classes {2,4,7,9,11}. A deliberately *incomplete* class set (not chromatic).
fn pentatonic() -> PitchMaterial {
    PitchMaterial {
        root: Pitch(40),
        intervals: vec![0, 3, 5, 7, 10],
    }
}

/// The pitch classes the pentatonic material allows.
fn allowed_classes() -> BTreeSet<u8> {
    [2_u8, 4, 7, 9, 11].into_iter().collect()
}

/// Wide 3-octave range E1..E4 (28..=64).
fn wide(bar_count: usize) -> GenerationConstraints {
    GenerationConstraints {
        bar_count,
        time_signature: TimeSignature {
            numerator: 4,
            denominator: 4,
        },
        tempo: Tempo(120.0),
        ticks_per_quarter: Ticks(480),
        pitch_lo: Pitch(28),
        pitch_hi: Pitch(64),
    }
}

const ALL_STRATEGIES: [GenerationStrategy; 5] = [
    GenerationStrategy::RhythmCopyPitchSubstitute,
    GenerationStrategy::MotifTransposeVariation,
    GenerationStrategy::ConstrainedRandomWalk,
    GenerationStrategy::ShuffleMotifs,
    GenerationStrategy::RepeatVariation,
];

fn pitches(score: &Score) -> Vec<u8> {
    score.tracks[0].voices[0]
        .event_groups
        .iter()
        .flat_map(|g| g.atoms.iter())
        .filter_map(|a| match a {
            AtomEvent::Note(n) => Some(n.pitch.0),
            AtomEvent::Rest(_) => None,
        })
        .collect()
}

fn request(strategy: GenerationStrategy, seed: u64) -> RuleGenerationRequest {
    RuleGenerationRequest {
        seed: GenerationSeed(seed),
        pitch_material: pentatonic(),
        constraints: wide(8),
        source_rhythms: Vec::new(),
        strategy,
    }
}

#[test]
fn every_strategy_stays_in_bounds_and_in_class_over_seeds() {
    for strategy in ALL_STRATEGIES {
        for seed in [1_u64, 2, 7, 42, 1000] {
            let candidate = generate(&request(strategy, seed)).expect("generate succeeds");
            for p in pitches(&candidate.score) {
                assert!(
                    (28..=64).contains(&p),
                    "{strategy:?} seed {seed}: pitch {p} out of [28,64]"
                );
                assert!(
                    allowed_classes().contains(&(p % 12)),
                    "{strategy:?} seed {seed}: pitch class {} not in the palette",
                    p % 12
                );
            }
        }
    }
}

#[test]
fn candidate_set_reaches_beyond_the_first_octave() {
    // The contract is REACHABILITY, not per-piece coverage: across the set,
    // pitches must span more than one octave above pitch_lo — the old mapper
    // pinned rhythm-copy/shuffle to [root, root+12).
    let set = generate_candidate_set(&SetRequest {
        seed: GenerationSeed(7),
        pitch_material: pentatonic(),
        constraints: wide(8),
        source_rhythms: Vec::new(),
        variants_per_strategy: 3,
        gesture: None,
    })
    .expect("set generation");

    let highest = set
        .iter()
        .flat_map(|c| pitches(&c.score))
        .max()
        .expect("the set has notes");
    assert!(
        highest > 28 + 12,
        "candidate set stays within the first octave (highest {highest})"
    );
}

#[test]
fn generation_is_deterministic_over_the_ladder() {
    for strategy in ALL_STRATEGIES {
        let a = generate(&request(strategy, 123)).expect("run a");
        let b = generate(&request(strategy, 123)).expect("run b");
        assert_eq!(
            pitches(&a.score),
            pitches(&b.score),
            "{strategy:?}: fixed request must be deterministic"
        );
    }
}

#[test]
fn narrow_range_still_generates_in_bounds() {
    // A sub-octave range must still work (degenerate ladder), never panic.
    let narrow = GenerationConstraints {
        pitch_lo: Pitch(40),
        pitch_hi: Pitch(45),
        ..wide(4)
    };
    for strategy in ALL_STRATEGIES {
        let req = RuleGenerationRequest {
            constraints: narrow,
            ..request(strategy, 5)
        };
        let candidate = generate(&req).expect("narrow generate succeeds");
        for p in pitches(&candidate.score) {
            assert!((40..=45).contains(&p), "{strategy:?}: pitch {p} out of narrow range");
        }
    }
}

#[test]
fn reversed_bounds_do_not_regress() {
    // pitch_lo > pitch_hi is normalised, not a panic or an empty part.
    let reversed = GenerationConstraints {
        pitch_lo: Pitch(64),
        pitch_hi: Pitch(28),
        ..wide(4)
    };
    let candidate = generate(&RuleGenerationRequest {
        constraints: reversed,
        ..request(GenerationStrategy::ConstrainedRandomWalk, 9)
    })
    .expect("reversed-bounds generate succeeds");
    assert!(!pitches(&candidate.score).is_empty(), "still produces notes");
    for p in pitches(&candidate.score) {
        assert!((28..=64).contains(&p), "pitch {p} in normalised range");
    }
}
