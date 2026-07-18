//! Characterization: S6's ranked output is exactly what it was before S7.
//!
//! S7 Slice A/B consumes a [`RankedSet`] and must never perturb the pass that
//! produced it. So these tests describe S6's observable output for a fixed
//! seed, and any future chain work that quietly reaches back into generation
//! fails here rather than in someone's ears.
//!
//! What "characterization" requires: the expectations must come from **before**
//! the change. Asking the pass to run twice and comparing the answers proves
//! the pass is repeatable, which is worth knowing but is not the claim — a pass
//! that S7 had perturbed would agree with itself just as happily. The invariant
//! checks (descending aggregate, one rationale entry per axis) carry the same
//! caveat: they describe a contract, not this pass's actual output. Both are
//! kept below, under names that say which of the two they are.
//!
//! The golden values are therefore recorded from a detached worktree at the
//! merge commit S7 branched from, and pasted in as literals. Nothing here may
//! recompute an expectation by calling the code under test.
//!
//! Re-recording: check the base out into a worktree of its own, print the
//! values from a throwaway test there, and paste. Never bless from this branch
//! — that turns the file back into "the code equals itself".
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

/// A recorded candidate: `(strategy, derived variant seed, aggregate bits)`.
type GoldenCandidate = (&'static str, u64, u64);

/// A recorded note: `(absolute start, duration, pitch, velocity)`.
type GoldenNote = (u32, u32, u8, u8);

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
            tempo: Tempo::from_bpm_integer(120).unwrap(),
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

// ── the golden: S6's output as recorded before S7 existed ────────────────────

/// The base this golden was recorded at: the merge commit S7 branched from.
const GOLDEN_BASE: &str = "a02355a2b371b8e71b2dcf7c63d9999721d14bfa";

/// `(policy id, policy version)`.
const GOLDEN_POLICY: (&str, u32) = ("generation_rerank", 1);

/// The winner's rerank axis labels, in order.
const GOLDEN_AXES: &[&str] = &[
    "internal_continuity",
    "ending_stability",
    "final_lengthening",
    "gap_fill",
    "quote_novelty",
    "ngram_novelty",
];

/// `(strategy, derived variant seed, rerank aggregate bits)`, in rank order.
///
/// The aggregate is pinned by [`f64::to_bits`], not by a rounded decimal: a
/// characterization test that compares within a tolerance cannot see the drift
/// it exists to catch.
///
/// Ranks 1–2, 3–4, 5–6 and 8–9 are exact ties, so this also records the rank
/// tie-break — ADR-0017 §7's ascending original index — as it actually fell,
/// rather than as the contract says it should.
const GOLDEN_RANKED: &[GoldenCandidate] = &[
    (
        "RhythmCopyPitchSubstitute",
        13_679_457_532_755_275_413,
        0x3FEC_4444_4444_4443,
    ),
    (
        "ConstrainedRandomWalk",
        14_141_660_008_401_062_150,
        0x3FEC_4444_4444_4443,
    ),
    (
        "MotifTransposeVariation",
        2_949_826_092_126_892_291,
        0x3FEB_3333_3333_3332,
    ),
    (
        "MotifTransposeVariation",
        13_441_012_166_899_430_009,
        0x3FEB_3333_3333_3332,
    ),
    (
        "RhythmCopyPitchSubstitute",
        17_996_036_147_140_579_893,
        0x3FEA_AAAA_AAAA_AAAA,
    ),
    (
        "RepeatVariation",
        701_532_786_141_963_250,
        0x3FEA_AAAA_AAAA_AAAA,
    ),
    (
        "ShuffleMotifs",
        6_349_198_060_258_255_764,
        0x3FEA_2222_2222_2221,
    ),
    (
        "ConstrainedRandomWalk",
        5_139_283_748_462_763_858,
        0x3FE9_9999_9999_9999,
    ),
    (
        "RepeatVariation",
        4_691_677_783_938_580_081,
        0x3FE9_9999_9999_9999,
    ),
    (
        "ShuffleMotifs",
        16_326_166_102_818_017_940,
        0x3FE9_1111_1111_1110,
    ),
];

/// The winner's bar count.
const GOLDEN_WINNER_BARS: usize = 4;

/// The winner's tick resolution.
const GOLDEN_WINNER_PPQ: u16 = 960;

/// `(absolute start, duration, pitch, velocity)` of every note the winner
/// holds, in the order the canonical model holds them — the fingerprint of the
/// actual music S6 produced, not merely of its score.
const GOLDEN_WINNER_NOTES: &[GoldenNote] = &[
    (0, 480, 62, 90),
    (480, 480, 64, 90),
    (960, 480, 65, 90),
    (1440, 480, 67, 90),
    (3840, 480, 65, 90),
    (4320, 480, 64, 90),
    (4800, 480, 62, 90),
    (5280, 480, 60, 90),
    (7680, 480, 62, 90),
    (8160, 480, 64, 90),
    (8640, 480, 65, 90),
    (9120, 480, 67, 90),
    (11520, 480, 65, 90),
    (12000, 480, 64, 90),
    (12480, 480, 62, 90),
    (12960, 480, 60, 90),
];

#[test]
fn the_ranked_set_matches_the_output_recorded_before_s7() {
    let set = ranked_candidates(&source(), None, &ask(), None).expect("the pass runs");
    assert_eq!(
        (set.policy.id, set.policy.version),
        GOLDEN_POLICY,
        "the rerank policy S6 shipped with, as recorded at {GOLDEN_BASE}",
    );
    let observed: Vec<(String, u64, u64)> = set
        .ranked
        .iter()
        .map(|c| {
            (
                format!("{:?}", c.value.strategy),
                c.value.seed.0,
                c.aggregate().to_bits(),
            )
        })
        .collect();
    let golden: Vec<(String, u64, u64)> = GOLDEN_RANKED
        .iter()
        .map(|&(s, seed, bits)| (s.to_owned(), seed, bits))
        .collect();
    assert_eq!(
        observed, golden,
        "candidate count, rank order, strategies, variant seeds and exact \
         aggregates, as recorded at {GOLDEN_BASE}",
    );
}

#[test]
fn the_axis_labels_match_the_order_recorded_before_s7() {
    let set = ranked_candidates(&source(), None, &ask(), None).expect("the pass runs");
    let winner = set.ranked.first().expect("a winner");
    let labels: Vec<&str> = winner.axes.iter().map(|a| a.label).collect();
    assert_eq!(
        labels, GOLDEN_AXES,
        "the chain reads these axes by position; their set and order are S6's contract",
    );
}

#[test]
fn the_winners_music_matches_the_score_recorded_before_s7() {
    // The chain slices this score into bars and copies its event groups. If S6
    // ever generates different notes for this seed, every downstream number in
    // the slice — the 3.3 baseline included — silently describes other music.
    let set = ranked_candidates(&source(), None, &ask(), None).expect("the pass runs");
    let winner = set.ranked.first().expect("a winner");
    assert_eq!(winner.value.score.master_bars.len(), GOLDEN_WINNER_BARS);
    assert_eq!(winner.value.score.ticks_per_quarter, GOLDEN_WINNER_PPQ);
    let notes: Vec<(u32, u32, u8, u8)> = winner
        .value
        .score
        .tracks
        .iter()
        .flat_map(|t| t.voices.iter())
        .flat_map(|v| v.event_groups.iter())
        .flat_map(|g| g.atoms.iter())
        .filter_map(|atom| match atom {
            AtomEvent::Note(n) => Some((n.absolute_start.0, n.duration.0, n.pitch.0, n.velocity.0)),
            AtomEvent::Rest(_) => None,
        })
        .collect();
    assert_eq!(
        notes, GOLDEN_WINNER_NOTES,
        "the winner's notes, as recorded at {GOLDEN_BASE}",
    );
}

#[test]
fn the_ranked_set_order_and_aggregates_are_repeatable() {
    let set = ranked_candidates(&source(), None, &ask(), None).expect("the pass runs");

    // The observable shape of the set: how many candidates, under what policy.
    assert_eq!(set.policy.id, "generation_rerank");
    assert_eq!(set.policy.version, 1);
    assert!(!set.ranked.is_empty(), "the pass produced candidates");

    // Every candidate's identity and score, in rank order. This is the tuple
    // the chain reads — but comparing it to a second run of the same code only
    // shows the pass agrees with itself. What it *was* before S7 is the golden
    // above; this is the weaker, still-worth-having claim of repeatability.
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
