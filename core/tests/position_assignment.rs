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
    event::{FretboardPosition, NotePosition, TechniqueSource, Tuning},
    midi::import_score,
    score::AtomEvent,
};

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
