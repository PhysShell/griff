// TDD red phase: Shuffle-only LadderWindow (arbiter 2026-07-12). The register
// A/B confirmed reachability but flagged a coherence regression isolated to
// ShuffleMotifs: drawing every note from the whole ladder blew the register up
// (wide-synthetic median span ~11→46, octave-leap share ~0→0.62). Fix: a
// deterministic per-candidate contiguous ladder window (≤ one octave span) for
// Shuffle only — the full ladder stays the source of reachability, one window
// is the locally-coherent subset one candidate uses.
//
// This increment touches ShuffleMotifs ONLY; the other four strategies are
// unchanged. No rerank axis, no octave-leap rejection, no weight change.
//
// References griff_core::pitch::{LadderWindow, RegisterStats} and
// ScaleLadder::octave_window, which do not exist yet, so the suite fails to
// compile until the green step.
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
    pitch::{PitchClassSet, PitchRange, RegisterStats, ScaleLadder},
    score::{AtomEvent, Score},
};

// ── LadderWindow primitive ─────────────────────────────────────────────────────

#[test]
fn octave_window_spans_at_most_one_octave() {
    // Chromatic ladder over 3 octaves: every selector's window spans ≤ 12
    // semitones and is never empty.
    let classes = PitchClassSet::new(0..12);
    let ladder = ScaleLadder::build(&PitchRange::new(Pitch(28), Pitch(64)), &classes).expect("ok");
    for selector in 0..64 {
        let w = ladder.octave_window(selector);
        assert!(w.len() >= 1, "selector {selector}: window never empty");
        let ps: Vec<u8> = w.pitches().iter().map(|p| p.0).collect();
        assert!(
            ps.last().unwrap() - ps.first().unwrap() <= 12,
            "selector {selector}: span {} > one octave",
            ps.last().unwrap() - ps.first().unwrap()
        );
    }
}

#[test]
fn octave_window_selectors_cover_the_full_ladder() {
    // Across selectors the windows reach both the lowest and the highest rung
    // — no systematic top/bottom bias.
    let classes = PitchClassSet::new([0, 3, 5, 7, 10]);
    let ladder = ScaleLadder::build(&PitchRange::new(Pitch(28), Pitch(64)), &classes).expect("ok");
    let mut union: BTreeSet<u8> = BTreeSet::new();
    for selector in 0..128 {
        for p in ladder.octave_window(selector).pitches() {
            union.insert(p.0);
        }
    }
    let rungs: Vec<u8> = ladder.pitches().iter().map(|p| p.0).collect();
    assert_eq!(
        union.iter().min(),
        rungs.first(),
        "windows reach the bottom rung"
    );
    assert_eq!(
        union.iter().max(),
        rungs.last(),
        "windows reach the top rung"
    );
}

#[test]
fn octave_window_handles_narrow_and_single_rung_ladders() {
    // A ladder narrower than an octave → the window is the whole ladder.
    let classes = PitchClassSet::new([0, 4, 7]);
    let narrow = ScaleLadder::build(&PitchRange::new(Pitch(48), Pitch(55)), &classes).expect("ok");
    assert_eq!(narrow.octave_window(3).len(), narrow.len());

    // A single-rung ladder → a one-pitch window.
    let one = ScaleLadder::build(
        &PitchRange::new(Pitch(47), Pitch(49)),
        &PitchClassSet::new([0]),
    )
    .expect("one C");
    let w = one.octave_window(9);
    assert_eq!(w.len(), 1);
    assert_eq!(w.pitches(), &[Pitch(48)]);
}

#[test]
fn register_stats_measure_interval_shape() {
    // Exact-octave line [40,52,64]: intervals 12, 12 — exactly an octave each,
    // so over-octave (> 12) is 0 while exact-octave (== 12) is all of them.
    let stats = RegisterStats::measure(&[40, 52, 64]);
    assert!(
        (stats.mean_abs_interval - 12.0).abs() < 1e-9,
        "mean interval 12"
    );
    assert_eq!(stats.max_abs_interval, 12, "max interval 12");
    assert!(
        (stats.over_octave_share - 0.0).abs() < 1e-9,
        "no interval > 12"
    );
    assert!(
        (stats.exact_octave_share - 1.0).abs() < 1e-9,
        "every interval == 12"
    );
    assert!(
        (stats.at_least_octave_share - 1.0).abs() < 1e-9,
        ">= 12 is all"
    );

    // A flat line has zero spread and zero leaps of any kind.
    let flat = RegisterStats::measure(&[50, 50, 50]);
    assert_eq!(flat.max_abs_interval, 0);
    assert!((flat.over_octave_share - 0.0).abs() < 1e-9);
    assert!((flat.exact_octave_share - 0.0).abs() < 1e-9);
    assert!((flat.at_least_octave_share - 0.0).abs() < 1e-9);
    assert!((flat.pitch_stddev - 0.0).abs() < 1e-9);
}

#[test]
fn register_stats_split_octave_families_on_mixed_intervals() {
    // Intervals 5, 12, 13: a below-octave step, an exact octave, and an
    // over-octave leap — the three octave shares must separate cleanly.
    let stats = RegisterStats::measure(&[50, 55, 67, 80]);
    assert_eq!(stats.max_abs_interval, 13);
    assert!((stats.mean_abs_interval - 10.0).abs() < 1e-9, "(5+12+13)/3");
    assert!(
        (stats.over_octave_share - 1.0 / 3.0).abs() < 1e-9,
        "only 13 is > 12"
    );
    assert!(
        (stats.exact_octave_share - 1.0 / 3.0).abs() < 1e-9,
        "only 12 is == 12"
    );
    assert!(
        (stats.at_least_octave_share - 2.0 / 3.0).abs() < 1e-9,
        "12 and 13 are >= 12"
    );
}

// ── anchor selection (unbiased) ────────────────────────────────────────────────

#[test]
fn octave_window_count_is_the_full_octave_anchor_count() {
    // The anchor count is the number of rungs that leave a full octave above
    // them (rungs ≤ top − 12), floored at 1 — for both a dense (chromatic) and
    // a sparse (pentatonic) palette.
    for classes in [
        PitchClassSet::new(0..12),
        PitchClassSet::new([0, 3, 5, 7, 10]),
    ] {
        let ladder =
            ScaleLadder::build(&PitchRange::new(Pitch(28), Pitch(64)), &classes).expect("ok");
        let rungs: Vec<u8> = ladder.pitches().iter().map(|p| p.0).collect();
        let top = *rungs.last().unwrap();
        let expected = rungs
            .iter()
            .filter(|&&p| p <= top.saturating_sub(12))
            .count()
            .max(1);
        assert_eq!(
            ladder.octave_window_count(),
            expected,
            "anchor count must be the full-octave anchor count"
        );
    }
}

#[test]
fn every_anchor_index_is_a_nonempty_octave_window() {
    let classes = PitchClassSet::new(0..12);
    let ladder = ScaleLadder::build(&PitchRange::new(Pitch(28), Pitch(64)), &classes).expect("ok");
    for anchor in 0..ladder.octave_window_count() {
        let w = ladder.octave_window(anchor);
        assert!(w.len() >= 1, "anchor {anchor}: window never empty");
        let ps: Vec<u8> = w.pitches().iter().map(|p| p.0).collect();
        assert!(
            ps.last().unwrap() - ps.first().unwrap() <= 12,
            "anchor {anchor}: span exceeds one octave"
        );
    }
}

#[test]
fn first_and_last_anchors_reach_low_and_high_ends() {
    let classes = PitchClassSet::new([0, 3, 5, 7, 10]);
    let ladder = ScaleLadder::build(&PitchRange::new(Pitch(28), Pitch(64)), &classes).expect("ok");
    let count = ladder.octave_window_count();
    assert_eq!(
        ladder.octave_window(0).pitches().first(),
        ladder.pitches().first(),
        "anchor 0 starts at the lowest rung"
    );
    assert_eq!(
        ladder.octave_window(count - 1).pitches().last(),
        ladder.pitches().last(),
        "the top anchor reaches the highest rung"
    );
}

#[test]
fn cycling_anchor_indices_visits_each_window_once() {
    let classes = PitchClassSet::new(0..12);
    let ladder = ScaleLadder::build(&PitchRange::new(Pitch(28), Pitch(64)), &classes).expect("ok");
    let count = ladder.octave_window_count();
    let starts: Vec<u8> = (0..count)
        .map(|a| ladder.octave_window(a).pitches()[0].0)
        .collect();
    let mut unique = starts.clone();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), count, "each anchor index is a distinct window");
    // `count` (one past the last valid index) wraps to anchor 0 — the only
    // modulo, so the caller passing `next_mod(count)` never double-mods.
    assert_eq!(ladder.octave_window(count).pitches()[0].0, starts[0]);
}

// ── Shuffle generation contract ─────────────────────────────────────────────────

fn chromatic() -> PitchMaterial {
    PitchMaterial {
        root: Pitch(28),
        intervals: (0..12).collect(),
    }
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

fn shuffle_request(seed: u64, constraints: GenerationConstraints) -> RuleGenerationRequest {
    RuleGenerationRequest {
        seed: GenerationSeed(seed),
        pitch_material: chromatic(),
        constraints,
        source_rhythms: Vec::new(),
        strategy: GenerationStrategy::ShuffleMotifs,
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

#[test]
fn shuffle_candidate_stays_within_one_octave() {
    for seed in [1_u64, 2, 7, 42, 99, 1000, 55_555] {
        let c = generate(&shuffle_request(seed, wide(8))).expect("generate");
        let ps = pitches(&c.score);
        assert!(!ps.is_empty(), "seed {seed}: no empty line");
        let (lo, hi) = (*ps.iter().min().unwrap(), *ps.iter().max().unwrap());
        assert!(
            hi - lo <= 12,
            "seed {seed}: candidate span {} exceeds one octave",
            hi - lo
        );
        for p in &ps {
            assert!((28..=64).contains(p), "seed {seed}: pitch {p} out of range");
            assert!(chromatic().pitch_classes().contains_pitch(Pitch(*p)));
        }
        // A one-octave window admits no strictly-over-octave leap.
        assert!(
            (RegisterStats::measure(&ps).over_octave_share - 0.0).abs() < 1e-9,
            "seed {seed}: over-octave share must be zero inside a window"
        );
    }
}

#[test]
fn shuffle_variants_cover_below_middle_and_above_first_octave() {
    // Union across a stable seed set must reach the bottom, the second octave,
    // and the top — reachability is retained, only per-candidate locality is
    // bounded. (Not every variant must hit the exact endpoints.)
    let mut union: BTreeSet<u8> = BTreeSet::new();
    for seed in 0..200_u64 {
        for p in pitches(
            &generate(&shuffle_request(seed, wide(8)))
                .expect("generate")
                .score,
        ) {
            union.insert(p);
        }
    }
    assert!(
        union.iter().any(|&p| p < 40),
        "reaches the first octave (28..40)"
    );
    assert!(
        union.iter().any(|&p| (40..52).contains(&p)),
        "reaches the second octave"
    );
    assert!(
        union.iter().any(|&p| p >= 52),
        "reaches above the second octave"
    );
}

#[test]
fn shuffle_is_deterministic() {
    let a = generate(&shuffle_request(321, wide(8))).expect("a");
    let b = generate(&shuffle_request(321, wide(8))).expect("b");
    assert_eq!(pitches(&a.score), pitches(&b.score));
}

#[test]
fn shuffle_narrow_ladder_under_one_octave() {
    let narrow = GenerationConstraints {
        pitch_lo: Pitch(40),
        pitch_hi: Pitch(45),
        ..wide(4)
    };
    let c = generate(&shuffle_request(5, narrow)).expect("narrow");
    let ps = pitches(&c.score);
    assert!(!ps.is_empty());
    for p in ps {
        assert!((40..=45).contains(&p), "narrow pitch {p} out of range");
    }
}

#[test]
fn shuffle_single_rung_ladder_repeats_one_pitch() {
    // Range [47,49] with palette {C}: exactly one in-class pitch (C=48).
    let one = GenerationConstraints {
        pitch_lo: Pitch(47),
        pitch_hi: Pitch(49),
        ..wide(4)
    };
    let req = RuleGenerationRequest {
        pitch_material: PitchMaterial {
            root: Pitch(48),
            intervals: vec![0],
        },
        ..shuffle_request(5, one)
    };
    let c = generate(&req).expect("single rung");
    let ps = pitches(&c.score);
    assert!(!ps.is_empty());
    assert!(ps.iter().all(|&p| p == 48), "single-rung line is all C");
}
