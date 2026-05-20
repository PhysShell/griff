// TDD red phase for S8 piano-roll view.
// Fails to compile until `griff_preview::piano_roll` is implemented.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::missing_assert_message,
    clippy::absolute_paths,
    clippy::missing_const_for_fn
)]

use griff_core::{
    event::{Bar, Event, Note, Phrase, Pitch, Tempo, Ticks, TimeSignature, Velocity},
    midi::{self, MidiSong, MidiTrack, Ppqn},
};
use griff_preview::piano_roll::{PianoRollError, PianoRollView};

// ── helpers ───────────────────────────────────────────────────────────────────

fn note_event(pitch: u8, duration: u32, velocity: u8) -> Event {
    Event::Note(Note {
        pitch: Pitch(pitch),
        duration: Ticks(duration),
        velocity: Velocity(velocity),
        articulation: None,
    })
}

fn single_bar(events: Vec<Event>) -> Bar {
    Bar {
        time_signature: TimeSignature {
            numerator: 4,
            denominator: 4,
        },
        tempo: Tempo(120.0),
        events,
    }
}

fn minimal_midi_bytes() -> Vec<u8> {
    let events = vec![
        note_event(60, 480, 80), // C4
        note_event(64, 480, 80), // E4
        note_event(67, 480, 80), // G4
    ];
    let song = MidiSong {
        ppqn: Ppqn(480),
        tracks: vec![MidiTrack {
            name: None,
            channel: 0,
            phrase: Phrase {
                bars: vec![single_bar(events)],
            },
        }],
    };
    midi::export(&song).expect("export minimal MIDI")
}

fn two_track_midi_bytes() -> Vec<u8> {
    let track0 = MidiTrack {
        name: None,
        channel: 0,
        phrase: Phrase {
            bars: vec![single_bar(vec![note_event(48, 480, 64)])], // C3
        },
    };
    let track1 = MidiTrack {
        name: None,
        channel: 1,
        phrase: Phrase {
            bars: vec![single_bar(vec![note_event(72, 240, 100)])], // C5
        },
    };
    let song = MidiSong {
        ppqn: Ppqn(480),
        tracks: vec![track0, track1],
    };
    midi::export(&song).expect("export two-track MIDI")
}

// ── construction ──────────────────────────────────────────────────────────────

#[test]
fn piano_roll_loads_midi_bytes() {
    let bytes = minimal_midi_bytes();
    let view = PianoRollView::from_midi_bytes(&bytes);
    assert!(view.is_ok(), "from_midi_bytes should succeed for valid MIDI");
}

#[test]
fn piano_roll_empty_bytes_returns_error() {
    let result = PianoRollView::from_midi_bytes(&[]);
    assert!(
        result.is_err(),
        "from_midi_bytes should fail for empty input"
    );
}

#[test]
fn piano_roll_corrupted_bytes_returns_error() {
    let result = PianoRollView::from_midi_bytes(b"not valid midi data");
    assert!(
        result.is_err(),
        "from_midi_bytes should fail for corrupted bytes"
    );
}

// ── note count ────────────────────────────────────────────────────────────────

#[test]
fn piano_roll_note_count_matches_input() {
    let bytes = minimal_midi_bytes();
    let view = PianoRollView::from_midi_bytes(&bytes).expect("load minimal MIDI");
    assert_eq!(
        view.notes.len(),
        3,
        "note count must match the three notes in the source phrase"
    );
}

#[test]
fn piano_roll_two_tracks_note_count() {
    let bytes = two_track_midi_bytes();
    let view = PianoRollView::from_midi_bytes(&bytes).expect("load two-track MIDI");
    assert_eq!(
        view.notes.len(),
        2,
        "two-track MIDI has two notes total"
    );
}

// ── pitch range ───────────────────────────────────────────────────────────────

#[test]
fn piano_roll_pitch_range_single_track() {
    let bytes = minimal_midi_bytes();
    let view = PianoRollView::from_midi_bytes(&bytes).expect("load minimal MIDI");
    assert_eq!(view.pitch_lo, 60, "pitch_lo must be 60 (C4)");
    assert_eq!(view.pitch_hi, 67, "pitch_hi must be 67 (G4)");
}

#[test]
fn piano_roll_pitch_range_two_tracks() {
    let bytes = two_track_midi_bytes();
    let view = PianoRollView::from_midi_bytes(&bytes).expect("load two-track MIDI");
    assert_eq!(view.pitch_lo, 48, "pitch_lo must be 48 (C3)");
    assert_eq!(view.pitch_hi, 72, "pitch_hi must be 72 (C5)");
}

// ── note fields ───────────────────────────────────────────────────────────────

#[test]
fn piano_roll_note_fields_preserved() {
    let bytes = minimal_midi_bytes();
    let view = PianoRollView::from_midi_bytes(&bytes).expect("load minimal MIDI");
    let first = view.notes.first().expect("has notes");
    assert_eq!(first.pitch, 60, "first note pitch must be 60");
    assert_eq!(first.velocity, 80, "first note velocity must be 80");
    assert!(first.duration_ticks > 0, "duration must be positive");
}

// ── error variant ─────────────────────────────────────────────────────────────

#[test]
fn piano_roll_error_is_debug() {
    let err = PianoRollView::from_midi_bytes(&[]).unwrap_err();
    let s = format!("{err:?}");
    assert!(!s.is_empty(), "PianoRollError must implement Debug");
}

#[test]
fn piano_roll_error_is_display() {
    let err = PianoRollView::from_midi_bytes(&[]).unwrap_err();
    let s = format!("{err}");
    assert!(!s.is_empty(), "PianoRollError must implement Display");
    let _ = PianoRollError::Empty; // ensure variant is accessible
}
