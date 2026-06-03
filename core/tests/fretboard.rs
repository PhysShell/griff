//! Red → tests for ADR-0018 Slice 1: fretboard position on the note + `Tuning`.
//!
//! Pins the additive, `Copy`-preserving half of the rich note model: a
//! `FretboardPosition` value, a per-`Track` `Tuning` (default Standard E,
//! ADR-0006), an optional `position` on `AtomNote`, and that MIDI import — which
//! cannot recover a tuning — defaults to Standard E. References API that does not
//! exist yet, so the suite fails to compile until the green step. No technique /
//! evidence work here (that is Slice 2, and may break `Copy`).

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::missing_assert_message,
    clippy::indexing_slicing,
    clippy::float_cmp
)]

use griff_core::{
    event::{FretboardPosition, Pitch, Ticks, Tuning, Velocity},
    midi::import_score,
    score::{AtomNote, Track, Voice},
};

const SIMPLE_4_4_MID: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../cli/tests/fixtures/simple_4_4.mid"
));

#[test]
fn fretboard_position_is_a_copy_value() {
    let pos = FretboardPosition { string: 6, fret: 3 };
    let copied = pos; // Copy, not move.
    assert_eq!(pos, copied);
    assert_eq!(pos.string, 6);
    assert_eq!(pos.fret, 3);
}

#[test]
fn standard_e_maps_string_and_fret_to_pitch() {
    let tuning = Tuning::standard_e();
    // String 1 = high E (E4 = 64), string 6 = low E (E2 = 40).
    assert_eq!(
        tuning.pitch_at(FretboardPosition { string: 1, fret: 0 }),
        Some(Pitch(64))
    );
    assert_eq!(
        tuning.pitch_at(FretboardPosition { string: 6, fret: 0 }),
        Some(Pitch(40))
    );
    // 5th fret of the low E is A2 (45).
    assert_eq!(
        tuning.pitch_at(FretboardPosition { string: 6, fret: 5 }),
        Some(Pitch(45))
    );
    // Out-of-range string (0 or > 6) maps to nothing.
    assert_eq!(
        tuning.pitch_at(FretboardPosition { string: 0, fret: 0 }),
        None
    );
    assert_eq!(
        tuning.pitch_at(FretboardPosition { string: 7, fret: 0 }),
        None
    );
}

#[test]
fn atom_note_carries_optional_position() {
    let with = AtomNote {
        absolute_start: Ticks(0),
        duration: Ticks(480),
        pitch: Pitch(64),
        velocity: Velocity(90),
        articulation: None,
        position: Some(FretboardPosition { string: 1, fret: 0 }),
    };
    let without = AtomNote {
        position: None,
        ..with
    };
    assert_eq!(
        with.position,
        Some(FretboardPosition { string: 1, fret: 0 })
    );
    assert_eq!(without.position, None);
}

#[test]
fn track_carries_tuning() {
    let track = Track {
        name: Some("Lead".to_owned()),
        channel: 0,
        voices: Vec::<Voice>::new(),
        tuning: Tuning::standard_e(),
    };
    assert_eq!(track.tuning, Tuning::standard_e());
}

#[test]
fn midi_import_defaults_to_standard_e_tuning() {
    // MIDI carries no tuning, so import must fall back to Standard E (ADR-0006).
    let score = import_score(SIMPLE_4_4_MID).expect("fixture imports");
    let track = score.tracks.first().expect("one track");
    assert_eq!(track.tuning, Tuning::standard_e());
}
