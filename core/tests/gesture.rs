//! Red → green tests for burst/rest gesture statistics
//! (`docs/audit/2026-06-melodic-closure-research.md` §3.5, §7.4 — the DGD
//! burst-and-rest case).
//!
//! Pins the public `gesture` API: `measure_gesture` derives `GestureStats`
//! from a score track — distributions, not content: burst lengths (maximal
//! note runs between gesture rests), rest placement against the quarter grid,
//! landing-degree share (bursts landing on the line's modal pitch class), and
//! burst-final lengthening (the closure-v1 normalisation: landing duration
//! against twice the mean, equal-to-mean reads 0.5). A *gesture rest* is at
//! least one quarter of silence in the melodic line after a sounded note
//! (the highest-pitch-per-onset convention shared with closure / novelty /
//! curate); the trailing gap to the span end counts, leading silence does
//! not (a rest belongs to the gesture before it).
//!
//! References `griff_core::gesture`, which does not exist yet, so the suite
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
    event::{NoteMarks, Pitch, Tempo, Ticks, TimeSignature, Tuning, Velocity},
    gesture::{measure_gesture, GestureError},
    score::{
        AtomEvent, AtomNote, EventGroup, EventGroupKind, LossReport, MasterBar, Score, Track, Voice,
    },
    slice::TickRange,
};

const PPQN: u16 = 480;
const QUARTER: u32 = 480;
const SIXTEENTH: u32 = 120;
const BAR: u32 = 1920; // 4/4 at 480 PPQN

/// A `bar_count`-bar 4/4 score whose single track plays exactly `notes`
/// (`(onset, duration, pitch)`).
fn score_with_notes(bar_count: usize, notes: &[(u32, u32, u8)]) -> Score {
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

    let event_groups = notes
        .iter()
        .map(|&(onset, duration, pitch)| EventGroup {
            kind: EventGroupKind::Single,
            atoms: vec![AtomEvent::Note(AtomNote {
                absolute_start: Ticks(onset),
                duration: Ticks(duration),
                pitch: Pitch::new(pitch).expect("valid pitch"),
                velocity: Velocity::new(90).expect("valid velocity"),
                marks: NoteMarks::empty(),
                position: None,
            })],
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

// ── errors ──────────────────────────────────────────────────────────────────────

#[test]
fn empty_score_is_an_error() {
    let mut score = score_with_notes(1, &[(0, QUARTER, 60)]);
    score.master_bars.clear();
    assert!(matches!(
        measure_gesture(&score, 0),
        Err(GestureError::EmptyScore)
    ));
}

#[test]
fn track_index_out_of_range_is_an_error() {
    let score = score_with_notes(1, &[(0, QUARTER, 60)]);
    assert!(matches!(
        measure_gesture(&score, 5),
        Err(GestureError::TrackIndexOutOfRange)
    ));
}

#[test]
fn a_track_with_no_notes_is_an_error() {
    let score = score_with_notes(1, &[]);
    assert!(matches!(
        measure_gesture(&score, 0),
        Err(GestureError::NoNotes)
    ));
}

// ── burst segmentation ──────────────────────────────────────────────────────────

#[test]
fn a_contiguous_run_is_one_burst_with_its_trailing_rest() {
    // 8 contiguous sixteenths, then half a bar of silence: the DGD flurry.
    let notes: Vec<(u32, u32, u8)> = (0..8).map(|i| (i * SIXTEENTH, SIXTEENTH, 60)).collect();
    let stats = measure_gesture(&score_with_notes(1, &notes), 0).expect("measure ok");

    assert_eq!(stats.note_count, 8);
    assert_eq!(stats.burst_count, 1);
    assert_eq!(stats.mean_burst_notes, 8.0);
    assert_eq!(stats.max_burst_notes, 8);
    assert_eq!(stats.rest_count, 1, "the trailing gap is a gesture rest");
    assert_eq!(stats.mean_rest_quarters, 2.0);
    assert_eq!(
        stats.rest_on_grid_share, 1.0,
        "the rest starts on the quarter grid"
    );
    assert_eq!(
        stats.mean_final_lengthening, 0.5,
        "landing equal to the mean duration reads 0.5 (closure-v1 convention)"
    );
}

#[test]
fn quarter_rests_split_bursts() {
    // Two 4-sixteenth bursts separated by a one-quarter rest, plus the
    // trailing one-quarter rest.
    let stats = measure_gesture(
        &score_with_notes(
            1,
            &[
                (0, SIXTEENTH, 60),
                (SIXTEENTH, SIXTEENTH, 62),
                (2 * SIXTEENTH, SIXTEENTH, 64),
                (3 * SIXTEENTH, SIXTEENTH, 65),
                (2 * QUARTER, SIXTEENTH, 60),
                (2 * QUARTER + SIXTEENTH, SIXTEENTH, 62),
                (2 * QUARTER + 2 * SIXTEENTH, SIXTEENTH, 64),
                (2 * QUARTER + 3 * SIXTEENTH, SIXTEENTH, 60),
            ],
        ),
        0,
    )
    .expect("measure ok");

    assert_eq!(stats.burst_count, 2);
    assert_eq!(stats.mean_burst_notes, 4.0);
    assert_eq!(stats.max_burst_notes, 4);
    assert_eq!(stats.rest_count, 2);
    assert_eq!(stats.mean_rest_quarters, 1.0);
    assert_eq!(stats.rest_on_grid_share, 1.0);
}

#[test]
fn sub_quarter_gaps_stay_inside_a_burst() {
    // Staccato sixteenths an eighth apart: the holes are phrasing, not rests.
    let stats = measure_gesture(
        &score_with_notes(
            1,
            &[
                (0, SIXTEENTH, 60),
                (360, SIXTEENTH, 62),
                (720, SIXTEENTH, 64),
                (1080, SIXTEENTH, 65),
            ],
        ),
        0,
    )
    .expect("measure ok");

    assert_eq!(
        stats.burst_count, 1,
        "240-tick holes do not split the burst"
    );
    assert_eq!(stats.max_burst_notes, 4);
    assert_eq!(stats.rest_count, 1, "only the trailing gap qualifies");
    assert_eq!(
        stats.rest_on_grid_share, 0.0,
        "the trailing rest starts off the quarter grid (tick 1200)"
    );
    assert_eq!(stats.mean_rest_quarters, 1.5);
}

#[test]
fn leading_silence_is_not_a_rest() {
    // A pickup bar: silence before the first note belongs to no gesture.
    let stats = measure_gesture(&score_with_notes(1, &[(2 * QUARTER, 2 * QUARTER, 60)]), 0)
        .expect("measure ok");
    assert_eq!(stats.burst_count, 1);
    assert_eq!(
        stats.rest_count, 0,
        "leading silence is unanswered; the note runs to the span end"
    );
    assert_eq!(stats.mean_rest_quarters, 0.0);
    assert_eq!(
        stats.rest_on_grid_share, 1.0,
        "no rests: vacuously predictable"
    );
}

// ── rest placement ──────────────────────────────────────────────────────────────

#[test]
fn rest_grid_share_mixes_on_and_off_grid_rests() {
    // First rest starts off-grid (tick 120), second on-grid (tick 1440).
    let stats = measure_gesture(
        &score_with_notes(1, &[(0, SIXTEENTH, 60), (2 * QUARTER, QUARTER, 60)]),
        0,
    )
    .expect("measure ok");
    assert_eq!(stats.burst_count, 2);
    assert_eq!(stats.rest_count, 2);
    assert_eq!(stats.rest_on_grid_share, 0.5);
    assert_eq!(
        stats.mean_rest_quarters, 1.375,
        "(840 + 480) ticks over two"
    );
}

// ── landing degree and final lengthening ────────────────────────────────────────

#[test]
fn modal_landing_share_counts_bursts_ending_on_the_modal_pitch_class() {
    // Pitch classes: 60 appears three times (modal); burst 1 lands on 65,
    // burst 2 lands on 60 → half the bursts land on the modal class.
    let stats = measure_gesture(
        &score_with_notes(
            1,
            &[
                (0, SIXTEENTH, 60),
                (SIXTEENTH, SIXTEENTH, 62),
                (2 * SIXTEENTH, SIXTEENTH, 65),
                (2 * QUARTER, SIXTEENTH, 62),
                (2 * QUARTER + SIXTEENTH, SIXTEENTH, 60),
                (2 * QUARTER + 2 * SIXTEENTH, SIXTEENTH, 60),
            ],
        ),
        0,
    )
    .expect("measure ok");
    assert_eq!(stats.burst_count, 2);
    assert_eq!(stats.modal_landing_share, 0.5);
}

#[test]
fn final_lengthening_reads_the_burst_landing() {
    // Durations 120, 120, 240: the landing is 1.5× the mean (160), and the
    // closure-v1 normalisation maps it to 240 / (2 × 160) = 0.75.
    let stats = measure_gesture(
        &score_with_notes(
            1,
            &[
                (0, SIXTEENTH, 60),
                (SIXTEENTH, SIXTEENTH, 60),
                (2 * SIXTEENTH, 2 * SIXTEENTH, 60),
            ],
        ),
        0,
    )
    .expect("measure ok");
    assert_eq!(stats.burst_count, 1);
    assert_eq!(stats.mean_final_lengthening, 0.75);
}

#[test]
fn chords_count_their_top_note() {
    // A chord contributes one line entry: its top pitch with that note's
    // duration (the closure / novelty convention).
    let stats = measure_gesture(
        &score_with_notes(
            1,
            &[
                (0, QUARTER, 60),
                (0, 2 * SIXTEENTH, 67),
                (QUARTER, QUARTER, 64),
            ],
        ),
        0,
    )
    .expect("measure ok");
    assert_eq!(
        stats.note_count, 2,
        "two onsets, the chord folded to its top"
    );
    assert_eq!(
        stats.burst_count, 1,
        "the 240-tick hole after the top note does not split"
    );
}

// ── determinism ─────────────────────────────────────────────────────────────────

#[test]
fn measurement_is_deterministic() {
    let notes: Vec<(u32, u32, u8)> = (0..8).map(|i| (i * SIXTEENTH, SIXTEENTH, 60)).collect();
    let score = score_with_notes(2, &notes);
    let a = measure_gesture(&score, 0).expect("measure ok");
    let b = measure_gesture(&score, 0).expect("measure ok");
    assert_eq!(a, b);
}
