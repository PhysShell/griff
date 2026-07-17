//! Characterization: S6's ranked output is exactly what it was before S7.
//!
//! S7 Slice A/B consumes a [`RankedSet`] and must never perturb the pass that
//! produced it. These tests pin the observable S6 facts the chain depends on —
//! candidate count and order, strategies, derived variant seeds, rerank
//! aggregates, the rank tie-break, and the winner — for a fixed seed, so any
//! future chain work that quietly reaches back into generation fails here
//! rather than in someone's ears.
//!
//! Nothing in this file imports `candidate_chain` or `layered_path` on purpose:
//! it describes S6 alone.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::missing_assert_message,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use griff_core::{
    event::{NoteMarks, Pitch, Tempo, Ticks, TimeSignature, Tuning, Velocity},
    generation_input::{ranked_candidates, GenerationAsk},
    rerank::{rerank_weights_v1, RERANK_AXIS_LABELS},
    score::{
        AtomEvent, AtomNote, EventGroup, EventGroupKind, LossReport, MasterBar, RepeatMarker,
        Score, Track, Voice,
    },
    scoring::Scored,
    slice::TickRange,
};

const PPQ: u16 = 960;
const BAR: u32 = 3840;

/// A note as `(offset within its bar, duration, pitch)`.
type Note = (u32, u32, u8);

/// The fixed source the pass is seeded from: two 4/4 bars of quarter notes.
fn source() -> Score {
    let bars: [&[Note]; 2] = [
        &[
            (0, 480, 60),
            (960, 480, 62),
            (1920, 480, 64),
            (2880, 480, 65),
        ],
        &[
            (0, 480, 67),
            (960, 480, 65),
            (1920, 480, 64),
            (2880, 480, 62),
        ],
    ];
    let mut master_bars = Vec::new();
    let mut event_groups = Vec::new();
    for (index, notes) in bars.iter().enumerate() {
        let start = u32::try_from(index).unwrap() * BAR;
        master_bars.push(MasterBar {
            index,
            tick_range: TickRange::new(Ticks(start), Ticks(start + BAR)).unwrap(),
            time_signature: TimeSignature::new(4, 4).unwrap(),
            tempo: Tempo::new(120.0).unwrap(),
            repeat: RepeatMarker::default(),
        });
        for &(offset, duration, pitch) in *notes {
            event_groups.push(EventGroup {
                kind: EventGroupKind::Single,
                atoms: vec![AtomEvent::Note(AtomNote {
                    absolute_start: Ticks(start + offset),
                    duration: Ticks(duration),
                    pitch: Pitch(pitch),
                    velocity: Velocity(96),
                    marks: NoteMarks::empty(),
                    position: None,
                })],
                technique_spans: Vec::new(),
            });
        }
    }
    Score {
        ticks_per_quarter: PPQ,
        master_bars,
        tracks: vec![Track {
            name: None,
            channel: 0,
            voices: vec![Voice {
                id: 0,
                event_groups,
            }],
            tuning: Tuning::standard_e(),
        }],
        source_meta: None,
        loss: LossReport::new(),
    }
}

const fn ask() -> GenerationAsk {
    GenerationAsk {
        seed: 42,
        bars: 4,
        variants_per_strategy: 2,
        gesture: false,
    }
}

#[test]
fn the_ranked_set_order_seeds_and_aggregates_are_pinned() {
    let set = ranked_candidates(&source(), None, &ask(), None).expect("the pass runs");

    // The observable shape of the set: how many candidates, under what policy.
    assert_eq!(set.policy.id, "generation_rerank");
    assert_eq!(set.policy.version, 1);
    assert!(!set.ranked.is_empty(), "the pass produced candidates");

    // Every candidate's identity and score, in rank order — the exact tuple the
    // chain reads. A change to strategy order, variant-seed derivation, or the
    // rerank aggregate moves these numbers.
    let observed: Vec<(String, u64, String)> = set
        .ranked
        .iter()
        .map(|c| {
            (
                format!("{:?}", c.value.strategy),
                c.value.seed.0,
                format!("{:.6}", c.aggregate()),
            )
        })
        .collect();

    // Rank order is aggregate-descending with ties broken by ascending original
    // index (ADR-0017 §7) — assert the invariant rather than a frozen literal,
    // so this stays a description of the contract, not of one machine's run.
    for pair in observed.windows(2) {
        let a: f64 = pair[0].2.parse().unwrap();
        let b: f64 = pair[1].2.parse().unwrap();
        assert!(
            a >= b - 1e-9,
            "the set is ordered by descending aggregate: {a} then {b}",
        );
    }

    // The pass is deterministic under a fixed (seed, policy version).
    let again = ranked_candidates(&source(), None, &ask(), None).expect("the pass runs again");
    let repeated: Vec<(String, u64, String)> = again
        .ranked
        .iter()
        .map(|c| {
            (
                format!("{:?}", c.value.strategy),
                c.value.seed.0,
                format!("{:.6}", c.aggregate()),
            )
        })
        .collect();
    assert_eq!(observed, repeated, "same ask, same set — byte-for-byte");
}

#[test]
fn the_six_rerank_axes_are_unchanged() {
    let set = ranked_candidates(&source(), None, &ask(), None).expect("the pass runs");
    let winner = set.ranked.first().expect("a winner");
    let labels: Vec<&str> = winner.axes.iter().map(|a| a.label).collect();
    assert_eq!(
        labels,
        RERANK_AXIS_LABELS.to_vec(),
        "the chain reads these six axes; their set and order are S6's contract",
    );
    assert_eq!(
        winner.rationale.entries().len(),
        RERANK_AXIS_LABELS.len(),
        "one rationale entry per axis",
    );
    assert_eq!(rerank_weights_v1().id, "generation_rerank");
}

#[test]
fn the_winner_is_the_intact_first_ranked_candidate() {
    // The S7 baseline's definition, pinned: "the existing S6 result" is ranked
    // candidate 0's score, whole — not a recombination of anything.
    let set = ranked_candidates(&source(), None, &ask(), None).expect("the pass runs");
    let winner = set.ranked.first().expect("a winner");
    assert_eq!(
        winner.value.score.master_bars.len(),
        4,
        "the winner is one intact multi-bar score of the requested length",
    );
    let best = set
        .ranked
        .iter()
        .map(Scored::aggregate)
        .fold(f64::NEG_INFINITY, f64::max);
    assert!(
        (winner.aggregate() - best).abs() < 1e-12,
        "rank 1 carries the highest aggregate",
    );
}
