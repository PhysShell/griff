//! Red → green tests for the closure axis v1
//! (`docs/audit/2026-06-melodic-closure-research.md` §7 item 2).
//!
//! Pins the public `closure` API: `closure_axes` measures how *finished* a
//! phrase feels — melodic closure in the music-cognition sense, not a Rust
//! closure — as four named ADR-0017 axes over a score track:
//!
//! - `internal_continuity` — the S4 boundary detector re-used as the
//!   generation-side referee: no spurious break inside the phrase;
//! - `ending_stability`   — landing degree relative to the `PitchMaterial`
//!   (simplified Krumhansl-inspired tier table);
//! - `final_lengthening`  — phrase-final lengthening of the landing note;
//! - `gap_fill`           — Narmour implication-realization mini-rules over
//!   the last melodic intervals.
//!
//! Plus the uniform `closure` v1 `WeightPolicy`. Pure, deterministic, no RNG.
//! References `griff_core::closure`, which does not exist yet, so the suite
//! fails to compile until the green step.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::missing_assert_message,
    clippy::indexing_slicing,
    clippy::missing_const_for_fn,
    clippy::float_cmp,
    clippy::arithmetic_side_effects,
    clippy::str_to_string
)]

use griff_core::{
    closure::{closure_axes, closure_weights_v1, ClosureError, CLOSURE_AXIS_LABELS},
    event::{NoteMarks, Pitch, Tempo, Ticks, TimeSignature, Tuning, Velocity},
    generate::PitchMaterial,
    score::{
        AtomEvent, AtomNote, AtomRest, EventGroup, EventGroupKind, LossReport, MasterBar, Score,
        Track, Voice,
    },
    scoring::{rank_indices, Scored},
    slice::TickRange,
};

const PPQN: u16 = 480;
const QUARTER: u32 = 480;
const BAR: u32 = 1920; // 4/4 at 480 PPQN

fn c_major() -> PitchMaterial {
    PitchMaterial {
        root: Pitch(60),
        intervals: vec![0, 2, 4, 5, 7, 9, 11],
    }
}

fn note(start: u32, dur: u32, pitch: u8) -> AtomEvent {
    AtomEvent::Note(AtomNote {
        absolute_start: Ticks(start),
        duration: Ticks(dur),
        pitch: Pitch::new(pitch).expect("valid pitch"),
        velocity: Velocity::new(90).expect("valid velocity"),
        marks: NoteMarks::empty(),
        position: None,
    })
}

fn rest(start: u32, dur: u32) -> AtomEvent {
    AtomEvent::Rest(AtomRest {
        absolute_start: Ticks(start),
        duration: Ticks(dur),
    })
}

/// A 4/4 score of `bar_count` master bars whose first voice holds `atoms`.
fn build_score(bar_count: usize, atoms: Vec<AtomEvent>) -> Score {
    let master_bars = (0..bar_count)
        .map(|i| {
            let start = u32::try_from(i).unwrap() * BAR;
            MasterBar {
                index: i,
                tick_range: TickRange::new(Ticks(start), Ticks(start + BAR)).expect("ordered"),
                time_signature: TimeSignature {
                    numerator: 4,
                    denominator: 4,
                },
                tempo: Tempo::new(120.0).expect("120 BPM"),
            }
        })
        .collect();

    let event_groups = atoms
        .into_iter()
        .map(|a| EventGroup {
            kind: EventGroupKind::Single,
            atoms: vec![a],
            technique_spans: Vec::new(),
        })
        .collect();

    Score {
        ticks_per_quarter: PPQN,
        master_bars,
        tracks: vec![Track {
            name: Some("A".to_string()),
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

/// A finished phrase: a contiguous arch landing long on the root.
///
/// Quarters 60 67 64 62 across bar 0, then a half-note 60 on the bar-1
/// downbeat. No rests, no internal breaks.
fn closed_phrase() -> Score {
    build_score(
        2,
        vec![
            note(0, QUARTER, 60),
            note(QUARTER, QUARTER, 67),
            note(2 * QUARTER, QUARTER, 64),
            note(3 * QUARTER, QUARTER, 62),
            note(BAR, 2 * QUARTER, 60),
        ],
    )
}

/// A broken-off stub: two sparse notes, a 1.5-bar hole, then a short
/// out-of-material flurry ending on an unresolved upward leap.
fn broken_stub() -> Score {
    build_score(
        3,
        vec![
            note(0, QUARTER, 60),
            note(QUARTER, QUARTER / 2, 66),
            rest(720, 3120), // hole up to the bar-2 downbeat
            note(2 * BAR, 120, 74),
            note(2 * BAR + 120, 120, 73),
            note(2 * BAR + 240, 120, 74),
            note(2 * BAR + 360, 120, 85),
        ],
    )
}

// ── axis labels and policy ────────────────────────────────────────────────────

#[test]
fn axis_labels_are_stable_and_ordered() {
    assert_eq!(
        CLOSURE_AXIS_LABELS,
        [
            "internal_continuity",
            "ending_stability",
            "final_lengthening",
            "gap_fill",
        ]
    );

    let axes = closure_axes(&closed_phrase(), 0, &c_major()).expect("axes");
    let labels: Vec<&str> = axes.iter().map(|a| a.label).collect();
    assert_eq!(labels, CLOSURE_AXIS_LABELS.to_vec());
}

#[test]
fn weights_v1_is_a_uniform_named_versioned_policy() {
    let policy = closure_weights_v1();
    assert_eq!(policy.id, "closure");
    assert_eq!(policy.version, 1);
    for label in CLOSURE_AXIS_LABELS {
        assert_eq!(policy.weight(label), 0.25, "uniform over four axes");
    }
    assert_eq!(policy.weight("no_such_axis"), 0.0);
}

// ── the closed phrase scores high on every axis ───────────────────────────────

#[test]
fn closed_phrase_scores_high_across_axes() {
    let axes = closure_axes(&closed_phrase(), 0, &c_major()).expect("axes");

    assert_eq!(
        axes.get("internal_continuity").unwrap(),
        1.0,
        "contiguous phrase has no internal break"
    );
    assert_eq!(
        axes.get("ending_stability").unwrap(),
        1.0,
        "lands on the root"
    );
    // Durations 480×4 + 960: mean 576, last 960 → 960 / (2·576) = 5/6.
    assert!((axes.get("final_lengthening").unwrap() - 5.0 / 6.0).abs() < 1e-12);
    assert_eq!(
        axes.get("gap_fill").unwrap(),
        0.8,
        "stepwise approach into the landing note"
    );
}

#[test]
fn broken_stub_scores_low_across_axes() {
    let axes = closure_axes(&broken_stub(), 0, &c_major()).expect("axes");

    // The 1.5-bar hole trips the S4 hard-rest rule (score 1.0), so
    // continuity collapses to 0.0 — any value clearly below the closed
    // phrase's 1.0 satisfies the contract.
    let continuity = axes.get("internal_continuity").unwrap();
    assert!(
        continuity < 0.5,
        "the mid-phrase hole is a detected internal break: {continuity}"
    );
    assert_eq!(
        axes.get("ending_stability").unwrap(),
        0.2,
        "lands outside the pitch material"
    );
    // Durations 480 240 120 120 120 120: mean 200, last 120 → 120/400.
    assert_eq!(axes.get("final_lengthening").unwrap(), 0.3);
    assert_eq!(
        axes.get("gap_fill").unwrap(),
        0.0,
        "ends on an unresolved upward leap"
    );
}

// ── ending-stability tier table ───────────────────────────────────────────────

/// Three contiguous quarters ending on `last`.
fn phrase_ending_on(last: u8) -> Score {
    build_score(
        1,
        vec![
            note(0, QUARTER, 62),
            note(QUARTER, QUARTER, 64),
            note(2 * QUARTER, QUARTER, last),
        ],
    )
}

#[test]
fn ending_stability_tiers() {
    let m = c_major();
    let stability = |last: u8| {
        closure_axes(&phrase_ending_on(last), 0, &m)
            .expect("axes")
            .get("ending_stability")
            .unwrap()
    };

    assert_eq!(stability(60), 1.0, "root");
    assert_eq!(stability(67), 0.8, "fifth, in material");
    assert_eq!(stability(64), 0.7, "third, in material");
    assert_eq!(stability(62), 0.5, "other in-material degree");
    assert_eq!(stability(61), 0.2, "outside the material");
}

#[test]
fn ending_stability_of_a_final_chord_takes_the_most_stable_note() {
    // Quarter 64, then a final two-note group 64+60 at the same onset:
    // the root inside the chord wins.
    let score = build_score(
        1,
        vec![
            note(0, QUARTER, 64),
            note(QUARTER, QUARTER, 64),
            note(2 * QUARTER, QUARTER, 60),
            note(2 * QUARTER, QUARTER, 64),
        ],
    );
    let axes = closure_axes(&score, 0, &c_major()).expect("axes");
    assert_eq!(axes.get("ending_stability").unwrap(), 1.0);
}

// ── gap-fill tiers (Narmour mini-rules) ───────────────────────────────────────

/// Contiguous quarters over `pitches`, one bar per four notes.
fn line(pitches: &[u8]) -> Score {
    let atoms = pitches
        .iter()
        .enumerate()
        .map(|(i, &p)| note(u32::try_from(i).unwrap() * QUARTER, QUARTER, p))
        .collect();
    let bars = pitches.len().div_ceil(4).max(1);
    build_score(bars, atoms)
}

#[test]
fn gap_fill_tiers() {
    let m = c_major();
    let gap_fill = |pitches: &[u8]| {
        closure_axes(&line(pitches), 0, &m)
            .expect("axes")
            .get("gap_fill")
            .unwrap()
    };

    assert_eq!(
        gap_fill(&[60, 72, 67]),
        1.0,
        "leap up, smaller reversal down — the gap is filled"
    );
    assert_eq!(gap_fill(&[64, 62, 60]), 0.8, "stepwise approach");
    assert_eq!(gap_fill(&[60, 62, 67]), 0.4, "medium same-direction leap");
    assert_eq!(gap_fill(&[60, 62, 74]), 0.0, "final unresolved leap");
    assert_eq!(gap_fill(&[60, 62]), 0.8, "two notes: stepwise tier");
    assert_eq!(gap_fill(&[60, 74]), 0.0, "two notes: leap tier");
    assert_eq!(gap_fill(&[60]), 0.5, "single note: neutral");
}

#[test]
fn gap_fill_reads_the_highest_pitch_per_onset() {
    // A two-note group (60+72) then 67: the melodic line is 72 → 67, a
    // medium descending leap — not the stepwise 60 → 67 reading.
    let score = build_score(
        1,
        vec![
            note(0, QUARTER, 60),
            note(0, QUARTER, 72),
            note(QUARTER, QUARTER, 67),
        ],
    );
    let axes = closure_axes(&score, 0, &c_major()).expect("axes");
    assert_eq!(axes.get("gap_fill").unwrap(), 0.4);
}

// ── final lengthening ─────────────────────────────────────────────────────────

#[test]
fn final_lengthening_is_half_for_uniform_durations() {
    // All quarters: last == mean → 0.5.
    let axes = closure_axes(&line(&[60, 62, 64, 60]), 0, &c_major()).expect("axes");
    assert_eq!(axes.get("final_lengthening").unwrap(), 0.5);
}

// ── ranking integration (ADR-0017 envelope) ───────────────────────────────────

#[test]
fn closed_phrase_outranks_the_broken_stub() {
    let m = c_major();
    let policy = closure_weights_v1();

    let scored: Vec<Scored<u32>> = [closed_phrase(), broken_stub()]
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let axes = closure_axes(s, 0, &m).expect("axes");
            Scored::new(u32::try_from(i).unwrap(), axes, &policy, None)
        })
        .collect();

    let order = rank_indices(&scored);
    assert_eq!(order[0], 0, "the finished phrase ranks first");
    assert!(scored[0].aggregate() > scored[1].aggregate());
    assert_eq!(scored[0].provenance.policy_id, "closure");
    assert_eq!(scored[0].provenance.policy_version, 1);
    assert_eq!(scored[0].provenance.seed, None, "purely analytic score");
}

// ── determinism ───────────────────────────────────────────────────────────────

#[test]
fn axes_are_deterministic() {
    let score = broken_stub();
    let m = c_major();
    let a = closure_axes(&score, 0, &m).expect("axes");
    let b = closure_axes(&score, 0, &m).expect("axes");
    let pairs: Vec<(&str, f64)> = a.iter().map(|x| (x.label, x.value)).collect();
    let again: Vec<(&str, f64)> = b.iter().map(|x| (x.label, x.value)).collect();
    assert_eq!(pairs, again);
}

// ── errors ────────────────────────────────────────────────────────────────────

#[test]
fn empty_score_is_an_error() {
    let score = build_score(0, Vec::new());
    assert_eq!(
        closure_axes(&score, 0, &c_major()).unwrap_err(),
        ClosureError::EmptyScore
    );
}

#[test]
fn track_index_out_of_range_is_an_error() {
    assert_eq!(
        closure_axes(&closed_phrase(), 5, &c_major()).unwrap_err(),
        ClosureError::TrackIndexOutOfRange
    );
}

#[test]
fn a_track_with_no_notes_is_an_error() {
    let score = build_score(1, vec![rest(0, BAR)]);
    assert_eq!(
        closure_axes(&score, 0, &c_major()).unwrap_err(),
        ClosureError::NoNotes
    );
}
