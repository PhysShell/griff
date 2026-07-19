//! Characterization of the structural-equality facts the exact semantic
//! comparator (S16 Phase 4-pre B1) builds on.
//!
//! The 4-pre A exact-scalar migration gave the score tree derived `Eq`. The
//! comparator's fast path and its "no auto-sort, no fuzzy matching" rules
//! lean on exactly these properties of the *existing* derives, pinned here
//! before the comparator exists:
//!
//! - equality is structural: a field-identical clone compares equal;
//! - equality is order-sensitive: reordered tracks / atoms / warnings are
//!   NOT equal — the comparator must report them, never re-sort them away;
//! - equality is per-field: one changed scalar breaks a `MasterBar`.
//!
//! Characterization of existing behaviour: no new API, green before commit.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::missing_assert_message,
    clippy::indexing_slicing
)]

use griff_core::{
    event::{NoteMarks, Pitch, Tempo, Ticks, TimeSignature, Tuning, Velocity},
    score::{
        AtomEvent, AtomNote, EventGroup, EventGroupKind, ImportWarning, MasterBar, RepeatMarker,
        Track, Voice,
    },
    slice::TickRange,
};

fn note(onset: u32, pitch: u8) -> AtomEvent {
    AtomEvent::Note(AtomNote {
        absolute_start: Ticks(onset),
        duration: Ticks(240),
        pitch: Pitch::new(pitch).expect("valid pitch"),
        velocity: Velocity::new(96).expect("valid velocity"),
        marks: NoteMarks::empty(),
        position: None,
    })
}

fn track(name: &str, atoms: Vec<AtomEvent>) -> Track {
    Track {
        name: Some(name.to_owned()),
        channel: 0,
        voices: vec![Voice {
            id: 0,
            event_groups: vec![EventGroup {
                kind: EventGroupKind::Single,
                atoms,
                technique_spans: Vec::new(),
            }],
        }],
        tuning: Tuning::standard_e(),
    }
}

#[test]
fn a_field_identical_clone_is_structurally_equal() {
    let original = track("a", vec![note(0, 40), note(240, 43)]);
    let clone = original.clone();
    assert_eq!(original, clone, "structural equality: clone == original");
}

#[test]
fn reordered_tracks_are_not_equal() {
    let a = track("a", vec![note(0, 40)]);
    let b = track("b", vec![note(0, 47)]);
    assert_ne!(
        vec![a.clone(), b.clone()],
        vec![b, a],
        "track order is a compared fact, not a normalization target"
    );
}

#[test]
fn reordered_atoms_are_not_equal() {
    let straight = EventGroup {
        kind: EventGroupKind::Single,
        atoms: vec![note(0, 40), note(240, 43)],
        technique_spans: Vec::new(),
    };
    let swapped = EventGroup {
        kind: EventGroupKind::Single,
        atoms: vec![note(240, 43), note(0, 40)],
        technique_spans: Vec::new(),
    };
    assert_ne!(
        straight, swapped,
        "atom order is a compared fact, not a normalization target"
    );
}

#[test]
fn reordered_loss_warnings_are_not_equal() {
    let first = ImportWarning::SmpteTimingUnsupported;
    let second = ImportWarning::Other("x".to_owned());
    assert_ne!(
        vec![first.clone(), second.clone()],
        vec![second, first],
        "warning order is part of the loss report's identity"
    );
}

#[test]
fn one_changed_scalar_breaks_master_bar_equality() {
    let bar = MasterBar {
        index: 0,
        tick_range: TickRange::new(Ticks(0), Ticks(1920)).expect("ordered"),
        time_signature: TimeSignature::new(4, 4).expect("4/4"),
        tempo: Tempo::from_bpm_integer(120).expect("120 BPM"),
        repeat: RepeatMarker::default(),
    };
    let faster = MasterBar {
        tempo: Tempo::from_bpm_integer(121).expect("121 BPM"),
        ..bar
    };
    assert_ne!(bar, faster, "tempo participates in structural equality");
    assert_eq!(
        bar,
        MasterBar { ..bar },
        "and an untouched copy stays equal"
    );
}
