//! Red → tests for `slice::extract_bars` (auto-split keystone, feature #2a).
//!
//! `griff split` slices a track into one chunk per phrase; each chunk is a
//! standalone, independently-measurable score over a contiguous run of bars.
//! `extract_bars` is that cut: whole bars re-indexed from 0, ticks rebased to 0,
//! notes outside the span dropped by onset. References API that does not exist
//! yet, so the suite fails to compile until the green step.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::missing_assert_message,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    clippy::missing_const_for_fn
)]

use griff_core::event::{NoteMarks, Pitch, Tempo, Ticks, TimeSignature, Tuning, Velocity};
use griff_core::score::{
    AtomEvent, AtomNote, EventGroup, EventGroupKind, LossReport, MasterBar, RepeatMarker, Score,
    Track, Voice,
};
use griff_core::slice::{extract_bars, TickRange};

const BAR: u32 = 1920; // 4/4 at 480 ppqn

fn note(start: u32, pitch: u8) -> AtomEvent {
    AtomEvent::Note(AtomNote {
        absolute_start: Ticks(start),
        duration: Ticks(480),
        pitch: Pitch(pitch),
        velocity: Velocity(90),
        marks: NoteMarks::empty(),
        position: None,
    })
}

fn bar(index: usize, start: u32) -> MasterBar {
    MasterBar {
        index,
        tick_range: TickRange::new(Ticks(start), Ticks(start + BAR)).expect("ordered"),
        time_signature: TimeSignature {
            numerator: 4,
            denominator: 4,
        },
        tempo: Tempo::new(120.0).expect("bpm"),
        repeat: RepeatMarker::default(),
    }
}

/// Four bars, one note on each downbeat (pitch 60 + bar index).
fn four_bar_score() -> Score {
    let notes = [note(0, 60), note(BAR, 61), note(2 * BAR, 62), note(3 * BAR, 63)];
    Score {
        ticks_per_quarter: 480,
        master_bars: vec![bar(0, 0), bar(1, BAR), bar(2, 2 * BAR), bar(3, 3 * BAR)],
        tracks: vec![Track {
            name: Some("g".to_owned()),
            channel: 0,
            voices: vec![Voice {
                id: 0,
                event_groups: notes
                    .into_iter()
                    .map(|a| EventGroup {
                        kind: EventGroupKind::Single,
                        atoms: vec![a],
                        technique_spans: Vec::new(),
                    })
                    .collect(),
            }],
            tuning: Tuning::standard_e(),
        }],
        source_meta: None,
        loss: LossReport::new(),
    }
}

fn onsets(score: &Score) -> Vec<(u32, u8)> {
    score.tracks[0]
        .voices[0]
        .event_groups
        .iter()
        .flat_map(|g| &g.atoms)
        .filter_map(|a| match a {
            AtomEvent::Note(n) => Some((n.absolute_start.0, n.pitch.0)),
            AtomEvent::Rest(_) => None,
        })
        .collect()
}

#[test]
fn extracts_a_contiguous_bar_run_reindexed_and_rebased() {
    let sub = extract_bars(&four_bar_score(), 1..3);

    // Two whole bars, re-indexed from 0 and rebased to tick 0.
    assert_eq!(sub.master_bars.len(), 2);
    assert_eq!((sub.master_bars[0].index, sub.master_bars[1].index), (0, 1));
    assert_eq!(sub.master_bars[0].tick_range.start.0, 0);
    assert_eq!(sub.master_bars[0].tick_range.end.0, BAR);
    assert_eq!(sub.master_bars[1].tick_range.start.0, BAR);
    assert_eq!(sub.master_bars[1].tick_range.end.0, 2 * BAR);
    assert_eq!(sub.ticks_per_quarter, 480);
    assert_eq!(sub.tracks[0].tuning, Tuning::standard_e());

    // Only the bar-1 and bar-2 notes survive, rebased; bars 0 and 3 dropped.
    assert_eq!(onsets(&sub), vec![(0, 61), (BAR, 62)]);
}

#[test]
fn empty_range_yields_no_bars_and_out_of_range_end_clamps() {
    // Empty selection → no bars (and no notes).
    let empty = extract_bars(&four_bar_score(), 2..2);
    assert_eq!(empty.master_bars.len(), 0);
    assert!(onsets(&empty).is_empty());

    // End past the last bar clamps to what exists (all four bars here).
    let all = extract_bars(&four_bar_score(), 0..9);
    assert_eq!(all.master_bars.len(), 4);
    assert_eq!(onsets(&all), vec![(0, 60), (BAR, 61), (2 * BAR, 62), (3 * BAR, 63)]);
}
