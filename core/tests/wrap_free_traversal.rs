// TDD red phase: wrap-free full-ladder traversal (arbiter 2026-07-12, commit
// B). After the Shuffle window fix all 21 residual production winners with a
// max interval > 12 were RhythmCopyPitchSubstitute: its ascending degree walk
// wrapped modulo the whole ladder (…top, 0, …) producing a 44–57 semitone
// downward jump. RepeatVariation had a latent candidate-level twin: its
// variation degree wrapped `base + 2` modulo the whole ladder.
//
// Fix (this increment): a reflecting degree cursor for RhythmCopy over the
// full ladder (…3 4 3 2 1 0 1…, no modulo), and a local two-degree
// displacement for RepeatVariation. No rerank axis, no weight change; the
// three other strategies (Shuffle/Motif/CRW) are untouched.
//
// Behaviour lives in griff_core::generate; this suite asserts the observable
// contract, so it fails on the current wrapping code until the green step.
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
        RhythmTemplate, RuleGenerationRequest,
    },
    pitch::{PitchRange, ScaleLadder},
    score::{AtomEvent, Score},
};

// ── helpers ───────────────────────────────────────────────────────────────────

fn material(intervals: Vec<u8>) -> PitchMaterial {
    PitchMaterial {
        root: Pitch(28),
        intervals,
    }
}

fn chromatic() -> PitchMaterial {
    material((0..12).collect())
}

fn pentatonic() -> PitchMaterial {
    material(vec![0, 3, 5, 7, 10])
}

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

fn request(
    strategy: GenerationStrategy,
    pm: PitchMaterial,
    seed: u64,
    constraints: GenerationConstraints,
) -> RuleGenerationRequest {
    RuleGenerationRequest {
        seed: GenerationSeed(seed),
        pitch_material: pm,
        constraints,
        source_rhythms: vec![RhythmTemplate::from_durations(&[Ticks(480); 4])],
        strategy,
    }
}

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

/// The ladder the generator uses for a material over the wide range.
fn ladder_of(pm: &PitchMaterial) -> Vec<u8> {
    let c = wide(1);
    ScaleLadder::build(&PitchRange::new(c.pitch_lo, c.pitch_hi), &pm.pitch_classes())
        .expect("in-class")
        .pitches()
        .iter()
        .map(|p| p.0)
        .collect()
}

fn max_abs_interval(ps: &[u8]) -> u8 {
    ps.windows(2).map(|w| w[0].abs_diff(w[1])).max().unwrap_or(0)
}

/// The longest run of a single repeated pitch.
fn longest_repeat(ps: &[u8]) -> usize {
    let mut best = 0;
    let mut run = 0;
    let mut prev = None;
    for &p in ps {
        if Some(p) == prev {
            run += 1;
        } else {
            run = 1;
            prev = Some(p);
        }
        best = best.max(run);
    }
    best
}

// ── RhythmCopy: reflecting traversal ────────────────────────────────────────────

#[test]
fn rhythm_copy_moves_by_adjacent_rungs_without_wrap() {
    for pm in [chromatic(), pentatonic()] {
        let ladder = ladder_of(&pm);
        let index_of =
            |p: u8| -> usize { ladder.iter().position(|&r| r == p).expect("pitch on ladder") };
        for seed in [1_u64, 2, 7, 42, 99, 1000, 55_555] {
            let c = generate(&request(
                GenerationStrategy::RhythmCopyPitchSubstitute,
                pm.clone(),
                seed,
                wide(8),
            ))
            .expect("generate");
            let ps = pitches(&c.score);
            assert!(!ps.is_empty(), "seed {seed}: no empty line");
            // Every consecutive step is one ladder rung — no high->low wrap.
            for w in ps.windows(2) {
                let (a, b) = (index_of(w[0]), index_of(w[1]));
                assert_eq!(
                    a.abs_diff(b),
                    1,
                    "seed {seed}: {} -> {} is not an adjacent rung (wrap?)",
                    w[0],
                    w[1]
                );
            }
            assert!(
                max_abs_interval(&ps) <= 12,
                "seed {seed}: interval > 12 (wrap)"
            );
            // Reflection never dwells on the top rung.
            assert!(longest_repeat(&ps) <= 1, "seed {seed}: prolonged clamp");
            for p in &ps {
                assert!((28..=64).contains(p));
                assert!(pm.pitch_classes().contains_pitch(Pitch(*p)));
            }
        }
    }
}

#[test]
fn rhythm_copy_union_reaches_low_middle_high() {
    let pm = pentatonic();
    let ladder = ladder_of(&pm);
    let (lo, hi) = (ladder[0], *ladder.last().unwrap());
    let mut union: BTreeSet<u8> = BTreeSet::new();
    for seed in 0..40 {
        let c = generate(&request(
            GenerationStrategy::RhythmCopyPitchSubstitute,
            pm.clone(),
            seed,
            wide(8),
        ))
        .expect("generate");
        union.extend(pitches(&c.score));
    }
    let mid = (u16::from(lo) + u16::from(hi)) / 2;
    assert!(union.iter().any(|&p| p <= lo + 4), "reaches the low register");
    assert!(
        union.iter().any(|&p| u16::from(p).abs_diff(mid) <= 3),
        "reaches the middle register"
    );
    assert!(union.iter().any(|&p| p >= hi - 4), "reaches the high register");
}

#[test]
fn rhythm_copy_is_deterministic() {
    let a = generate(&request(
        GenerationStrategy::RhythmCopyPitchSubstitute,
        chromatic(),
        123,
        wide(8),
    ))
    .expect("a");
    let b = generate(&request(
        GenerationStrategy::RhythmCopyPitchSubstitute,
        chromatic(),
        123,
        wide(8),
    ))
    .expect("b");
    assert_eq!(pitches(&a.score), pitches(&b.score));
}

// ── RepeatVariation: local displacement ─────────────────────────────────────────

#[test]
fn repeat_variation_stays_local_without_wrap() {
    // A wide seed sweep so a base degree near the top (the latent wrap case,
    // ~16/500 candidates) is certainly hit.
    for pm in [chromatic(), pentatonic()] {
        for seed in 0..256_u64 {
            let c = generate(&request(
                GenerationStrategy::RepeatVariation,
                pm.clone(),
                seed,
                wide(8),
            ))
            .expect("generate");
            let ps = pitches(&c.score);
            assert!(!ps.is_empty(), "seed {seed}: no empty line");
            for p in &ps {
                assert!((28..=64).contains(p), "seed {seed}: pitch {p} out of range");
                assert!(pm.pitch_classes().contains_pitch(Pitch(*p)));
            }
            // No full-ladder wrap: the varied last note is a *local* move from
            // the base, never a multi-octave jump.
            assert!(
                max_abs_interval(&ps) <= 12,
                "seed {seed}: interval > 12 (variation wrap)"
            );
        }
    }
}

#[test]
fn repeat_variation_narrow_ladders_work() {
    // Two-rung and one-rung ladders must not panic and stay in bounds.
    for (lo, hi, classes) in [(48_u8, 51_u8, vec![0_u8, 7]), (48, 49, vec![0])] {
        let pm = PitchMaterial {
            root: Pitch(48),
            intervals: classes,
        };
        let constraints = GenerationConstraints {
            pitch_lo: Pitch(lo),
            pitch_hi: Pitch(hi),
            ..wide(4)
        };
        let c = generate(&request(
            GenerationStrategy::RepeatVariation,
            pm.clone(),
            5,
            constraints,
        ))
        .expect("generate");
        for p in pitches(&c.score) {
            assert!((lo..=hi).contains(&p), "pitch {p} out of [{lo},{hi}]");
        }
    }
}
