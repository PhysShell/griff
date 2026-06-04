//! Red → tests for ADR-0018 Slice 2: per-note technique marks (`NoteMark`).
//!
//! Pins the multi-technique, `Copy`-preserving note model: a `NoteMark` set
//! (`NoteMarks`, a bitset) replacing `AtomNote`'s single `Option<Articulation>`,
//! and that the feature layer counts a note as articulated when it carries any
//! mark. References API that does not exist yet, so the suite fails to compile
//! until the green step. Evidence + the `SpanTechnique` split are Slice 2b.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::missing_assert_message,
    clippy::indexing_slicing
)]

use griff_core::{
    event::{NoteMark, NoteMarks, Pitch, Ticks, Velocity},
    feature::voice_features,
    score::{AtomEvent, AtomNote, EventGroup, EventGroupKind, Voice},
};

fn note(pitch: u8, marks: NoteMarks) -> AtomEvent {
    AtomEvent::Note(AtomNote {
        absolute_start: Ticks(0),
        duration: Ticks(480),
        pitch: Pitch(pitch),
        velocity: Velocity(90),
        marks,
        position: None,
    })
}

#[test]
fn note_marks_empty_insert_contains() {
    let mut marks = NoteMarks::empty();
    assert!(marks.is_empty());
    assert!(!marks.contains(NoteMark::Accent));

    marks.insert(NoteMark::Accent);
    marks.insert(NoteMark::Ghost);
    assert!(!marks.is_empty());
    assert!(marks.contains(NoteMark::Accent));
    assert!(marks.contains(NoteMark::Ghost));
    assert!(!marks.contains(NoteMark::Staccato));
    assert_eq!(marks.len(), 2);
}

#[test]
fn note_marks_co_occur_and_iterate() {
    // A note can be, at once, an accented pinch harmonic — the whole point.
    let marks = NoteMarks::empty()
        .with(NoteMark::Accent)
        .with(NoteMark::HarmonicPinch);
    let collected: Vec<NoteMark> = marks.iter().collect();
    assert_eq!(collected, vec![NoteMark::Accent, NoteMark::HarmonicPinch]);
}

#[test]
fn atom_note_carries_marks() {
    let plain = note(60, NoteMarks::empty());
    let marked = note(60, NoteMarks::empty().with(NoteMark::Tap));
    let AtomEvent::Note(p) = plain else {
        panic!("note")
    };
    let AtomEvent::Note(m) = marked else {
        panic!("note")
    };
    assert!(p.marks.is_empty());
    assert!(m.marks.contains(NoteMark::Tap));
}

#[test]
fn feature_counts_notes_carrying_any_mark() {
    let voice = Voice {
        id: 0,
        event_groups: vec![
            EventGroup {
                kind: EventGroupKind::Single,
                atoms: vec![note(60, NoteMarks::empty().with(NoteMark::Accent))],
                technique_spans: Vec::new(),
            },
            EventGroup {
                kind: EventGroupKind::Single,
                atoms: vec![note(62, NoteMarks::empty())],
                technique_spans: Vec::new(),
            },
        ],
    };
    let vf = voice_features(&voice).expect("features");
    assert_eq!(vf.note_count, 2);
    assert_eq!(vf.articulated_note_count, 1, "only the marked note counts");
}
