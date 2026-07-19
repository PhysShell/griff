//! Characterization of the ADR-0020 `NormalizedScore` projection as the
//! v1 normalized-musical policy (S16 Phase 4-pre B2).
//!
//! `NormalizedMusicalDiff` v1 does not change `normalize`; it names the
//! projection's *current* behavior as a versioned policy
//! (`adr-0020-normalized-musical`, version 1). These tests pin what that
//! behavior is today, so any future widening is a deliberate
//! `policy_version = 2`, never a silent drift:
//!
//! - rests are ignored;
//! - group kinds are not preserved (a chord and loose singles project the
//!   same);
//! - technique spans lose their ranges and evidence (name labels at the
//!   covered onsets only);
//! - fretboard positions lose their evidence;
//! - repeat markers are not projected;
//! - source metadata is not projected;
//! - the existing canonical order applies (import order does not matter).
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
    dump::normalize,
    event::{
        ConfidenceBps, FretboardPosition, NoteMarks, NotePosition, Pitch, SpanTechnique,
        TechniqueEvidence, Tempo, Ticks, TimeSignature, Tuning, Velocity,
    },
    score::{
        AtomEvent, AtomNote, AtomRest, EventGroup, EventGroupKind, LossReport, MasterBar,
        RepeatMarker, Score, SourceMeta, TechniqueSpan, Track, Voice,
    },
    slice::TickRange,
};

fn tick_range(start: u32, end: u32) -> TickRange {
    TickRange::new(Ticks(start), Ticks(end)).expect("ordered range")
}

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

fn one_track_score(groups: Vec<EventGroup>) -> Score {
    Score {
        ticks_per_quarter: 480,
        master_bars: vec![MasterBar {
            index: 0,
            tick_range: tick_range(0, 1920),
            time_signature: TimeSignature::new(4, 4).expect("4/4"),
            tempo: Tempo::from_bpm_integer(120).expect("120 BPM"),
            repeat: RepeatMarker::default(),
        }],
        tracks: vec![Track {
            name: Some("t".to_owned()),
            channel: 0,
            voices: vec![Voice {
                id: 0,
                event_groups: groups,
            }],
            tuning: Tuning::standard_e(),
        }],
        source_meta: None,
        loss: LossReport::new(),
    }
}

fn single(atoms: Vec<AtomEvent>) -> EventGroup {
    EventGroup {
        kind: EventGroupKind::Single,
        atoms,
        technique_spans: Vec::new(),
    }
}

#[test]
fn rests_are_ignored_by_the_projection() {
    let without = one_track_score(vec![single(vec![note(0, 40)])]);
    let with_rest = one_track_score(vec![single(vec![
        note(0, 40),
        AtomEvent::Rest(AtomRest {
            absolute_start: Ticks(480),
            duration: Ticks(240),
        }),
    ])]);
    assert_eq!(
        normalize(&without),
        normalize(&with_rest),
        "a rest leaves no trace in the v1 projection"
    );
}

#[test]
fn group_kinds_are_not_preserved() {
    let chord = one_track_score(vec![EventGroup {
        kind: EventGroupKind::Chord,
        atoms: vec![note(0, 40), note(0, 47)],
        technique_spans: Vec::new(),
    }]);
    let singles = one_track_score(vec![single(vec![note(0, 40)]), single(vec![note(0, 47)])]);
    assert_eq!(
        normalize(&chord),
        normalize(&singles),
        "a chord and loose simultaneous singles project identically"
    );
}

#[test]
fn span_ranges_and_evidence_are_flattened_to_name_labels() {
    let explicit = one_track_score(vec![EventGroup {
        kind: EventGroupKind::Single,
        atoms: vec![note(0, 40)],
        technique_spans: vec![TechniqueSpan {
            technique: SpanTechnique::PalmMute,
            tick_range: tick_range(0, 480),
            evidence: TechniqueEvidence::explicit(),
        }],
    }]);
    // Same technique, different range end (still covering the onset) and
    // inferred evidence: the projection cannot tell them apart.
    let inferred = one_track_score(vec![EventGroup {
        kind: EventGroupKind::Single,
        atoms: vec![note(0, 40)],
        technique_spans: vec![TechniqueSpan {
            technique: SpanTechnique::PalmMute,
            tick_range: tick_range(0, 960),
            evidence: TechniqueEvidence::inferred(ConfidenceBps::HALF),
        }],
    }]);
    assert_eq!(
        normalize(&explicit),
        normalize(&inferred),
        "span range detail and evidence are not v1 facts"
    );
    let projected = normalize(&explicit);
    assert_eq!(
        projected.tracks[0].bars[0].voices[0].notes[0].spans,
        vec!["palm_mute".to_owned()],
        "what survives is the technique's name label at the covered onset"
    );
}

#[test]
fn position_evidence_is_dropped() {
    let position = FretboardPosition { string: 6, fret: 0 };
    let mut explicit = one_track_score(vec![single(vec![note(0, 40)])]);
    match &mut explicit.tracks[0].voices[0].event_groups[0].atoms[0] {
        AtomEvent::Note(n) => n.position = Some(NotePosition::explicit(position)),
        AtomEvent::Rest(_) => panic!("fixture note"),
    }
    let mut inferred = one_track_score(vec![single(vec![note(0, 40)])]);
    match &mut inferred.tracks[0].voices[0].event_groups[0].atoms[0] {
        AtomEvent::Note(n) => {
            n.position = Some(NotePosition::inferred(position, ConfidenceBps::HALF));
        }
        AtomEvent::Rest(_) => panic!("fixture note"),
    }
    assert_eq!(
        normalize(&explicit),
        normalize(&inferred),
        "only (string, fret) survives; its evidence does not"
    );
}

#[test]
fn repeat_markers_are_not_projected() {
    let plain = one_track_score(vec![single(vec![note(0, 40)])]);
    let mut repeated = one_track_score(vec![single(vec![note(0, 40)])]);
    repeated.master_bars[0].repeat = RepeatMarker {
        start: true,
        play_count: 2,
    };
    assert_eq!(
        normalize(&plain),
        normalize(&repeated),
        "repeat barlines are not v1 facts"
    );
}

#[test]
fn source_metadata_is_not_projected() {
    let plain = one_track_score(vec![single(vec![note(0, 40)])]);
    let mut tagged = one_track_score(vec![single(vec![note(0, 40)])]);
    tagged.source_meta = Some(SourceMeta {
        format: Some("GP5".to_owned()),
    });
    assert_eq!(
        normalize(&plain),
        normalize(&tagged),
        "source metadata is not a v1 fact"
    );
}

#[test]
fn the_canonical_order_erases_import_order() {
    let straight = one_track_score(vec![single(vec![note(0, 40), note(240, 43)])]);
    let swapped = one_track_score(vec![single(vec![note(240, 43), note(0, 40)])]);
    assert_eq!(
        normalize(&straight),
        normalize(&swapped),
        "notes land in canonical (onset, string, fret, pitch) order"
    );
}
