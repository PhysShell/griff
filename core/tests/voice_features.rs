//! Characterization tests for canonical-model feature extraction.
//!
//! Exercises `feature::{voice_features, VoiceFeatures}` over the canonical model
//! (`Voice → EventGroup → AtomEvent`), part of ADR-0011 (retire the legacy
//! linear model).
//!
//! Note: there is no `bar_count` — bars are a score-level concept
//! (`MasterBar`), not a property of a `Voice`.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::missing_assert_message,
    clippy::absolute_paths,
    clippy::missing_const_for_fn
)]

use griff_core::event::{Articulation, Pitch, Ticks, ValidationError, Velocity};
use griff_core::feature::{voice_features, PitchRange, VelocityRange, VoiceFeatures};
use griff_core::score::{AtomEvent, AtomNote, AtomRest, EventGroup, EventGroupKind, Voice};

fn note_group(
    start: u32,
    duration: u32,
    pitch: u8,
    velocity: u8,
    art: Option<Articulation>,
) -> EventGroup {
    EventGroup {
        kind: EventGroupKind::Single,
        atoms: vec![AtomEvent::Note(AtomNote {
            absolute_start: Ticks(start),
            duration: Ticks(duration),
            pitch: Pitch(pitch),
            velocity: Velocity(velocity),
            articulation: art,
            position: None,
        })],
        technique_spans: Vec::new(),
    }
}

fn rest_group(start: u32, duration: u32) -> EventGroup {
    EventGroup {
        kind: EventGroupKind::Single,
        atoms: vec![AtomEvent::Rest(AtomRest {
            absolute_start: Ticks(start),
            duration: Ticks(duration),
        })],
        technique_spans: Vec::new(),
    }
}

#[test]
fn voice_features_extracts_counts_spans_and_duration() {
    let voice = Voice {
        id: 0,
        event_groups: vec![
            note_group(0, 120, 64, 80, None),
            rest_group(120, 60),
            note_group(180, 240, 60, 100, Some(Articulation::PalmMute)),
            note_group(420, 120, 67, 70, None),
        ],
    };

    assert_eq!(
        voice_features(&voice),
        Ok(VoiceFeatures {
            event_count: 4,
            note_count: 3,
            rest_count: 1,
            articulated_note_count: 1,
            total_duration: Ticks(540),
            pitch_range: Some(PitchRange {
                lowest: Pitch(60),
                highest: Pitch(67),
            }),
            velocity_range: Some(VelocityRange {
                lowest: Velocity(70),
                highest: Velocity(100),
            }),
        }),
    );
}

#[test]
fn voice_features_reports_empty_voice() {
    let voice = Voice {
        id: 0,
        event_groups: Vec::new(),
    };

    assert_eq!(
        voice_features(&voice),
        Ok(VoiceFeatures {
            event_count: 0,
            note_count: 0,
            rest_count: 0,
            articulated_note_count: 0,
            total_duration: Ticks(0),
            pitch_range: None,
            velocity_range: None,
        }),
    );
}

#[test]
fn voice_features_keeps_spans_empty_for_rest_only_voice() {
    let voice = Voice {
        id: 0,
        event_groups: vec![rest_group(0, 120), rest_group(120, 360)],
    };

    assert_eq!(
        voice_features(&voice),
        Ok(VoiceFeatures {
            event_count: 2,
            note_count: 0,
            rest_count: 2,
            articulated_note_count: 0,
            total_duration: Ticks(480),
            pitch_range: None,
            velocity_range: None,
        }),
    );
}

#[test]
fn voice_features_counts_all_atoms_in_a_group() {
    // A chord group carries several simultaneous atoms; every atom is counted.
    let voice = Voice {
        id: 0,
        event_groups: vec![EventGroup {
            kind: EventGroupKind::Chord,
            atoms: vec![
                AtomEvent::Note(AtomNote {
                    absolute_start: Ticks(0),
                    duration: Ticks(240),
                    pitch: Pitch(52),
                    velocity: Velocity(90),
                    articulation: None,
                    position: None,
                }),
                AtomEvent::Note(AtomNote {
                    absolute_start: Ticks(0),
                    duration: Ticks(240),
                    pitch: Pitch(59),
                    velocity: Velocity(90),
                    articulation: None,
                    position: None,
                }),
            ],
            technique_spans: Vec::new(),
        }],
    };

    let features = voice_features(&voice).expect("valid voice");
    assert_eq!(features.note_count, 2);
    assert_eq!(features.event_count, 2);
    assert_eq!(
        features.pitch_range,
        Some(PitchRange {
            lowest: Pitch(52),
            highest: Pitch(59),
        }),
    );
}

#[test]
fn voice_features_reports_duration_overflow() {
    let voice = Voice {
        id: 0,
        event_groups: vec![rest_group(0, u32::MAX), rest_group(0, 1)],
    };

    assert_eq!(
        voice_features(&voice),
        Err(ValidationError::DurationOverflow),
    );
}
