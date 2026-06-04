//! Red → characterization tests for canonical-model bar classification.
//!
//! Ports the legacy `bar_features(&Bar)` behaviour onto the canonical model:
//! features are computed over a [`Voice`]'s note atoms sliced by a
//! [`MasterBar`]'s tick range. `classify_bar` is unchanged (it already operates
//! on pure `BarFeatures`). Part of ADR-0011 (retire the legacy linear model).
//!
//! References `classify::bar_features_in_range`, which does not exist yet, so the
//! suite fails to compile until the green step lands.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::missing_assert_message,
    clippy::indexing_slicing,
    clippy::missing_const_for_fn
)]

use std::slice::from_ref;

use griff_core::{
    classify::{
        bar_features_across_voices, bar_features_in_range, classify_bar, BarClass, BarFeatures,
    },
    event::{NoteMarks, Pitch, Ticks, Velocity},
    score::{AtomEvent, AtomNote, AtomRest, EventGroup, EventGroupKind, Voice},
    slice::TickRange,
};

fn range(start: u32, end: u32) -> TickRange {
    TickRange::new(Ticks(start), Ticks(end)).expect("ordered range")
}

fn note_atom(start: u32, dur: u32, pitch: u8, vel: u8) -> AtomEvent {
    AtomEvent::Note(AtomNote {
        absolute_start: Ticks(start),
        duration: Ticks(dur),
        pitch: Pitch::new(pitch).expect("valid pitch"),
        velocity: Velocity::new(vel).expect("valid velocity"),
        marks: NoteMarks::empty(),
        position: None,
    })
}

fn rest_atom(start: u32, dur: u32) -> AtomEvent {
    AtomEvent::Rest(AtomRest {
        absolute_start: Ticks(start),
        duration: Ticks(dur),
    })
}

/// A single-voice with each atom in its own `Single` group.
fn voice(atoms: Vec<AtomEvent>) -> Voice {
    Voice {
        id: 0,
        event_groups: atoms
            .into_iter()
            .map(|a| EventGroup {
                kind: EventGroupKind::Single,
                atoms: vec![a],
                technique_spans: Vec::new(),
            })
            .collect(),
    }
}

// ── bar_features_in_range ───────────────────────────────────────────────────────

#[test]
fn empty_voice_gives_zeros() {
    let v = voice(vec![]);
    assert_eq!(
        bar_features_in_range(&v, range(0, 1920)),
        BarFeatures {
            note_count: 0,
            avg_velocity: 0,
            pitch_span: 0,
        }
    );
}

#[test]
fn rests_only_gives_zeros() {
    let v = voice(vec![rest_atom(0, 480), rest_atom(480, 480)]);
    assert_eq!(
        bar_features_in_range(&v, range(0, 1920)),
        BarFeatures {
            note_count: 0,
            avg_velocity: 0,
            pitch_span: 0,
        }
    );
}

#[test]
fn single_note_in_range() {
    let v = voice(vec![note_atom(0, 480, 60, 90)]);
    let f = bar_features_in_range(&v, range(0, 1920));
    assert_eq!(f.note_count, 1);
    assert_eq!(f.avg_velocity, 90);
    assert_eq!(f.pitch_span, 0);
}

#[test]
fn computes_avg_velocity() {
    // two notes: vel 60 + 100 → avg 80
    let v = voice(vec![
        note_atom(0, 240, 60, 60),
        note_atom(240, 240, 62, 100),
    ]);
    assert_eq!(bar_features_in_range(&v, range(0, 1920)).avg_velocity, 80);
}

#[test]
fn computes_pitch_span() {
    // E2 (40) to E5 (64) = 24 semitones
    let v = voice(vec![note_atom(0, 240, 40, 80), note_atom(240, 240, 64, 80)]);
    assert_eq!(bar_features_in_range(&v, range(0, 1920)).pitch_span, 24);
}

#[test]
fn ignores_rests_in_counts() {
    let v = voice(vec![
        note_atom(0, 240, 60, 80),
        rest_atom(240, 240),
        note_atom(480, 240, 62, 80),
    ]);
    assert_eq!(bar_features_in_range(&v, range(0, 1920)).note_count, 2);
}

#[test]
fn slices_only_atoms_whose_onset_is_in_range() {
    // Two bars' worth of notes; the second bar starts at 1920.
    let v = voice(vec![
        note_atom(0, 480, 40, 80),
        note_atom(960, 480, 50, 80),
        note_atom(1920, 480, 60, 80),
        note_atom(2880, 480, 70, 80),
    ]);
    // Bar 0 = [0, 1920): pitches 40, 50 → span 10, count 2.
    let bar0 = bar_features_in_range(&v, range(0, 1920));
    assert_eq!(bar0.note_count, 2);
    assert_eq!(bar0.pitch_span, 10);
    // Bar 1 = [1920, 3840): pitches 60, 70 → span 10, count 2.
    let bar1 = bar_features_in_range(&v, range(1920, 3840));
    assert_eq!(bar1.note_count, 2);
    assert_eq!(bar1.pitch_span, 10);
}

#[test]
fn onset_on_range_end_is_excluded() {
    // Half-open [start, end): a note starting exactly at `end` belongs to the next bar.
    let v = voice(vec![note_atom(1920, 480, 60, 80)]);
    assert_eq!(bar_features_in_range(&v, range(0, 1920)).note_count, 0);
    assert_eq!(bar_features_in_range(&v, range(1920, 3840)).note_count, 1);
}

#[test]
fn counts_all_atoms_of_a_chord_group() {
    // A chord: one group with three simultaneous note atoms.
    let chord = EventGroup {
        kind: EventGroupKind::Chord,
        atoms: vec![
            note_atom(0, 480, 40, 80),
            note_atom(0, 480, 47, 80),
            note_atom(0, 480, 52, 80),
        ],
        technique_spans: Vec::new(),
    };
    let v = Voice {
        id: 0,
        event_groups: vec![chord],
    };
    let f = bar_features_in_range(&v, range(0, 1920));
    assert_eq!(f.note_count, 3);
    assert_eq!(f.pitch_span, 12);
}

// ── classify_bar over canonical features ────────────────────────────────────────

#[test]
fn classify_canonical_sparse_heavy_is_breakdown() {
    // 4 heavy notes in a bar → Breakdown.
    let v = voice(vec![
        note_atom(0, 480, 40, 100),
        note_atom(480, 480, 40, 100),
        note_atom(960, 480, 52, 100),
        note_atom(1440, 480, 40, 100),
    ]);
    let f = bar_features_in_range(&v, range(0, 1920));
    assert_eq!(classify_bar(f), BarClass::Breakdown);
}

#[test]
fn classify_canonical_empty_bar_is_unknown() {
    let v = voice(vec![]);
    let f = bar_features_in_range(&v, range(0, 1920));
    assert_eq!(classify_bar(f), BarClass::Unknown);
}

// ── bar_features_across_voices ──────────────────────────────────────────────────

#[test]
fn across_voices_aggregates_every_voice() {
    // One logical part split across two voices: a low voice and a high voice.
    let low = voice(vec![
        note_atom(0, 480, 40, 100),
        note_atom(480, 480, 40, 100),
    ]);
    let high = voice(vec![note_atom(0, 480, 64, 80), note_atom(960, 480, 67, 80)]);

    let f = bar_features_across_voices(&[low, high], range(0, 1920));

    assert_eq!(f.note_count, 4, "notes from both voices are counted");
    assert_eq!(
        f.pitch_span, 27,
        "span covers the lowest to the highest (40..67)"
    );
    assert_eq!(f.avg_velocity, 90, "(100 + 100 + 80 + 80) / 4");
}

#[test]
fn across_voices_of_one_voice_equals_the_single_voice_wrapper() {
    let v = voice(vec![note_atom(0, 480, 40, 90), note_atom(960, 480, 52, 70)]);
    assert_eq!(
        bar_features_across_voices(from_ref(&v), range(0, 1920)),
        bar_features_in_range(&v, range(0, 1920)),
    );
}

#[test]
fn across_voices_skips_a_silent_first_voice() {
    // The exact gap the fix closes: voice 0 empty, content lives in voice 1.
    let empty = voice(vec![]);
    let busy = voice(vec![
        note_atom(0, 240, 40, 100),
        note_atom(240, 240, 43, 100),
        note_atom(480, 240, 45, 100),
        note_atom(720, 240, 47, 100),
    ]);
    let f = bar_features_across_voices(&[empty, busy], range(0, 1920));
    assert_eq!(f.note_count, 4);
    assert_ne!(
        classify_bar(f),
        BarClass::Unknown,
        "voice 1 is not invisible"
    );
}
