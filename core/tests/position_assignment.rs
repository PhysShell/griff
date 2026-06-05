//! Red → tests for ADR-0019 P1b: MIDI import assigns inferred positions.
//!
//! Pins `NotePosition` (a `FretboardPosition` plus its `TechniqueEvidence`) and
//! that MIDI import runs the fingering DP to fill each note's position, marked
//! `InferredFromMidi` (the first real producer of that path). References API that
//! does not exist yet, so the suite fails to compile until the green step.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::missing_assert_message,
    clippy::indexing_slicing,
    clippy::float_cmp,
    clippy::missing_const_for_fn
)]

use griff_core::{
    event::{
        FretboardPosition, NoteMarks, NotePosition, Pitch, TechniqueSource, Ticks, Tuning, Velocity,
    },
    fretboard::{assign_inferred_positions, FingeringWeights, STANDARD_MAX_FRET},
    midi::import_score,
    score::{AtomEvent, AtomNote, EventGroup, EventGroupKind, Voice},
};

fn note_at(onset: u32, pitch: u8) -> EventGroup {
    EventGroup {
        kind: EventGroupKind::Single,
        atoms: vec![AtomEvent::Note(AtomNote {
            absolute_start: Ticks(onset),
            duration: Ticks(480),
            pitch: Pitch(pitch),
            velocity: Velocity(90),
            marks: NoteMarks::empty(),
            position: None,
        })],
        technique_spans: Vec::new(),
    }
}

const SIMPLE_4_4_MID: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../cli/tests/fixtures/simple_4_4.mid"
));

#[test]
fn note_position_explicit_is_full_confidence() {
    let np = NotePosition::explicit(FretboardPosition { string: 1, fret: 0 });
    assert_eq!(np.position, FretboardPosition { string: 1, fret: 0 });
    assert_eq!(np.evidence.source, TechniqueSource::Explicit);
    assert_eq!(np.evidence.confidence, 1.0);
}

#[test]
fn note_position_inferred_carries_confidence() {
    let np = NotePosition::inferred(FretboardPosition { string: 2, fret: 3 }, 0.5);
    assert_eq!(np.evidence.source, TechniqueSource::InferredFromMidi);
    assert_eq!(np.evidence.confidence, 0.5);
}

#[test]
fn midi_import_assigns_inferred_positions() {
    let score = import_score(SIMPLE_4_4_MID).expect("fixture imports");
    let voice = &score.tracks[0].voices[0];
    let first = voice
        .event_groups
        .iter()
        .flat_map(|g| &g.atoms)
        .find_map(|a| match a {
            AtomEvent::Note(n) => Some(*n),
            AtomEvent::Rest(_) => None,
        })
        .expect("the track has a note");

    let np = first.position.expect("MIDI import infers a position");
    // MIDI has no source-of-truth position, so it must be marked inferred.
    assert_eq!(np.evidence.source, TechniqueSource::InferredFromMidi);
    // The inferred (string, fret) must actually produce the note's pitch.
    assert_eq!(
        Tuning::standard_e().pitch_at(np.position),
        Some(first.pitch)
    );
}

#[test]
fn chord_notes_are_left_unknown_not_misvoiced() {
    // E4 (64) and G4 (67) at the same tick: a chord. The monophonic DP must not
    // place both on string 1 — leave them unknown (ADR-0019 §7, chords deferred).
    let mut voice = Voice {
        id: 0,
        event_groups: vec![note_at(0, 64), note_at(0, 67)],
    };
    let oor = assign_inferred_positions(
        &mut voice,
        &Tuning::standard_e(),
        &FingeringWeights::v1(),
        STANDARD_MAX_FRET,
    );
    assert_eq!(oor, 0, "chord notes are not out-of-range losses");
    for g in &voice.event_groups {
        for a in &g.atoms {
            if let AtomEvent::Note(n) = a {
                assert!(n.position.is_none(), "a chord note must be left unknown");
            }
        }
    }
}

#[test]
fn out_of_range_pitch_is_counted_and_left_none() {
    // 30 is below the low-E open string: no playable position.
    let mut voice = Voice {
        id: 0,
        event_groups: vec![note_at(0, 30)],
    };
    let oor = assign_inferred_positions(
        &mut voice,
        &Tuning::standard_e(),
        &FingeringWeights::v1(),
        STANDARD_MAX_FRET,
    );
    assert_eq!(oor, 1, "the out-of-range note is reported as a loss");
    let AtomEvent::Note(n) = &voice.event_groups[0].atoms[0] else {
        panic!("note");
    };
    assert!(n.position.is_none());
}
