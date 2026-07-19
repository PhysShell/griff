//! Red → tests for the auto-derived `syncopated` rhythm tag (#75). It sets
//! [`SwancoreTag::Syncopated`] from note-onset *displacement*: an off-beat "and"
//! onset that anticipates a beat carrying no onset of its own. Steady runs (every
//! beat struck) score zero, so they are not tagged. References
//! `griff_core::syncopation`, which does not exist yet, so the suite fails to
//! compile until the green step.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::missing_assert_message,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use griff_core::corpus::SwancoreTag;
use griff_core::event::{NoteMarks, Pitch, Tempo, Ticks, TimeSignature, Tuning, Velocity};
use griff_core::score::{
    AtomEvent, AtomNote, EventGroup, EventGroupKind, LossReport, MasterBar, RepeatMarker, Score,
    Track, Voice,
};
use griff_core::slice::TickRange;
use griff_core::syncopation::derive_syncopated;

const PPQN: u16 = 480;
const BAR: u32 = 1920; // 4/4 at 480 PPQN

fn note_at(onset: u32) -> EventGroup {
    EventGroup {
        kind: EventGroupKind::Single,
        atoms: vec![AtomEvent::Note(AtomNote {
            absolute_start: Ticks(onset),
            duration: Ticks(120),
            pitch: Pitch(60),
            velocity: Velocity(90),
            marks: NoteMarks::empty(),
            position: None,
        })],
        technique_spans: Vec::new(),
    }
}

/// A 4/4, 480-PPQN score of `bars` bars whose single voice strikes a note at each
/// absolute tick in `onsets`.
fn score_with_onsets(bars: usize, onsets: &[u32]) -> Score {
    let master_bars = (0..bars)
        .map(|i| {
            let start = u32::try_from(i).unwrap() * BAR;
            MasterBar {
                index: i,
                tick_range: TickRange::new(Ticks(start), Ticks(start + BAR)).expect("ordered"),
                time_signature: TimeSignature {
                    numerator: 4,
                    denominator: 4,
                },
                tempo: Tempo::from_bpm_integer(120).expect("120 BPM"),
                repeat: RepeatMarker::default(),
            }
        })
        .collect();
    Score {
        ticks_per_quarter: PPQN,
        master_bars,
        tracks: vec![Track {
            name: Some("g".to_owned()),
            channel: 0,
            voices: vec![Voice {
                id: 0,
                event_groups: onsets.iter().map(|&o| note_at(o)).collect(),
            }],
            tuning: Tuning::standard_e(),
        }],
        source_meta: None,
        loss: LossReport::new(),
    }
}

#[test]
fn steady_eighth_run_is_not_syncopated() {
    // Half the onsets are off-beat, but every beat is still struck → nothing is
    // displaced.
    let onsets = [0, 240, 480, 720, 960, 1200, 1440, 1680];
    assert!(derive_syncopated(&score_with_onsets(1, &onsets), 0).is_empty());
}

#[test]
fn on_beat_quarters_are_not_syncopated() {
    assert!(derive_syncopated(&score_with_onsets(1, &[0, 480, 960, 1440]), 0).is_empty());
}

#[test]
fn anticipated_beats_derive_syncopated() {
    // The "and of 2" (720) and "and of 3" (1200) anticipate beats 3 and 4, which
    // carry no onset → 2 of 4 beats displaced (0.5).
    let tags = derive_syncopated(&score_with_onsets(1, &[0, 720, 1200]), 0);
    assert_eq!(tags, vec![SwancoreTag::Syncopated]);
}

#[test]
fn quarter_of_beats_displaced_meets_threshold() {
    // One displaced beat of four = 0.25, exactly the (inclusive) threshold.
    let tags = derive_syncopated(&score_with_onsets(1, &[0, 720]), 0);
    assert_eq!(tags, vec![SwancoreTag::Syncopated]);
}

#[test]
fn light_displacement_below_threshold_is_not_tagged() {
    // One displaced beat across two bars (8 beats) = 0.125 < 0.25. Bar 0: every
    // beat struck. Bar 1: the "and of 3" (2640) anticipates beat 3 (2880).
    let onsets = [0, 480, 960, 1440, 1920, 2400, 2640, 3360];
    assert!(derive_syncopated(&score_with_onsets(2, &onsets), 0).is_empty());
}

#[test]
fn out_of_range_track_is_empty() {
    assert!(derive_syncopated(&score_with_onsets(1, &[0, 720, 1200]), 9).is_empty());
}

#[test]
fn a_score_with_no_bars_is_empty() {
    // No master timeline → nothing to place onsets against.
    let mut score = score_with_onsets(1, &[0, 720, 1200]);
    score.master_bars.clear();
    assert!(derive_syncopated(&score, 0).is_empty());
}

#[test]
fn is_deterministic() {
    let score = score_with_onsets(1, &[0, 720, 1200]);
    assert_eq!(derive_syncopated(&score, 0), derive_syncopated(&score, 0));
}
