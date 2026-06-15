//! Golden snapshot of the normalized score dump (ADR-0020).
//!
//! Pins the canonical JSON projection a Guitar-Pro-import validator compares
//! against a reference parser. Re-bless deliberately:
//! `INSTA_UPDATE=always cargo test -p griff-core --test dump_golden`.

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
        FretboardPosition, NoteMark, NoteMarks, NotePosition, Pitch, SpanTechnique,
        TechniqueEvidence, Tempo, Ticks, TimeSignature, Tuning, Velocity,
    },
    score::{
        AtomEvent, AtomNote, EventGroup, EventGroupKind, LossReport, MasterBar, Score,
        TechniqueSpan, Track, Voice,
    },
    slice::TickRange,
};

fn pitch(value: u8) -> Pitch {
    Pitch::new(value).expect("valid pitch")
}

/// A one-bar, one-voice score with a positioned + accented note under a palm
/// mute and a plain note, to exercise every normalized note field.
fn sample_score() -> Score {
    let range = TickRange::new(Ticks(0), Ticks(1920)).expect("ordered range");

    let positioned = AtomEvent::Note(AtomNote {
        absolute_start: Ticks(0),
        duration: Ticks(480),
        pitch: pitch(64),
        velocity: Velocity::new(80).expect("valid velocity"),
        marks: NoteMarks::empty().with(NoteMark::Accent),
        position: Some(NotePosition::explicit(FretboardPosition { string: 1, fret: 0 })),
    });
    let plain = AtomEvent::Note(AtomNote {
        absolute_start: Ticks(480),
        duration: Ticks(480),
        pitch: pitch(67),
        velocity: Velocity::new(90).expect("valid velocity"),
        marks: NoteMarks::empty(),
        position: None,
    });

    let group = EventGroup {
        kind: EventGroupKind::Single,
        atoms: vec![positioned, plain],
        technique_spans: vec![TechniqueSpan {
            technique: SpanTechnique::PalmMute,
            tick_range: TickRange::new(Ticks(0), Ticks(960)).expect("ordered range"),
            evidence: TechniqueEvidence::explicit(),
        }],
    };

    Score {
        ticks_per_quarter: 480,
        master_bars: vec![MasterBar {
            index: 0,
            tick_range: range,
            time_signature: TimeSignature::new(4, 4).expect("valid meter"),
            tempo: Tempo::new(120.0).expect("valid tempo"),
        }],
        tracks: vec![Track {
            name: Some("Guitar".to_owned()),
            channel: 0,
            voices: vec![Voice {
                id: 0,
                event_groups: vec![group],
            }],
            tuning: Tuning::standard_e(),
        }],
        source_meta: None,
        loss: LossReport::new(),
    }
}

#[test]
fn normalized_dump_golden() {
    let normalized = normalize(&sample_score());
    let json = serde_json::to_string_pretty(&normalized).expect("serialize");
    insta::assert_snapshot!(json);
}

/// One note in a bar, as a `Single` event group.
fn one_note_group(onset: u32, value: u8) -> EventGroup {
    EventGroup {
        kind: EventGroupKind::Single,
        atoms: vec![AtomEvent::Note(AtomNote {
            absolute_start: Ticks(onset),
            duration: Ticks(480),
            pitch: pitch(value),
            velocity: Velocity::new(80).expect("valid velocity"),
            marks: NoteMarks::empty(),
            position: None,
        })],
        technique_spans: vec![],
    }
}

#[test]
fn bar_voices_sorted_by_id_regardless_of_import_order() {
    let score = Score {
        ticks_per_quarter: 480,
        master_bars: vec![MasterBar {
            index: 0,
            tick_range: TickRange::new(Ticks(0), Ticks(1920)).expect("ordered range"),
            time_signature: TimeSignature::new(4, 4).expect("valid meter"),
            tempo: Tempo::new(120.0).expect("valid tempo"),
        }],
        tracks: vec![Track {
            name: None,
            channel: 0,
            // Deliberately out of id order: a deterministic dump must not echo it.
            voices: vec![
                Voice {
                    id: 1,
                    event_groups: vec![one_note_group(0, 60)],
                },
                Voice {
                    id: 0,
                    event_groups: vec![one_note_group(480, 62)],
                },
            ],
            tuning: Tuning::standard_e(),
        }],
        source_meta: None,
        loss: LossReport::new(),
    };

    let normalized = normalize(&score);
    let ids: Vec<u8> = normalized.tracks[0].bars[0]
        .voices
        .iter()
        .map(|voice| voice.id)
        .collect();
    assert_eq!(
        ids,
        vec![0, 1],
        "bar voices must be sorted by id, not by import order"
    );
}

/// A positioned chord note at `onset` on `string`/`fret` sounding `value`.
fn chord_note(onset: u32, string: u8, fret: u8, value: u8) -> AtomEvent {
    AtomEvent::Note(AtomNote {
        absolute_start: Ticks(onset),
        duration: Ticks(480),
        pitch: pitch(value),
        velocity: Velocity::new(80).expect("valid velocity"),
        marks: NoteMarks::empty(),
        position: Some(NotePosition::explicit(FretboardPosition { string, fret })),
    })
}

#[test]
fn same_onset_notes_sorted_by_string_before_pitch() {
    // A two-note chord at one onset. In standard tuning string 1 is the highest
    // pitch, so string order and pitch order disagree: ADR-0020 mandates
    // `(bar, voice, onset, string)`, not pitch. The notes are fed in pitch order
    // (what a pitch-first key would already bless), so the assertion passes only
    // when the dump truly keys on string.
    let score = Score {
        ticks_per_quarter: 480,
        master_bars: vec![MasterBar {
            index: 0,
            tick_range: TickRange::new(Ticks(0), Ticks(1920)).expect("ordered range"),
            time_signature: TimeSignature::new(4, 4).expect("valid meter"),
            tempo: Tempo::new(120.0).expect("valid tempo"),
        }],
        tracks: vec![Track {
            name: None,
            channel: 0,
            voices: vec![Voice {
                id: 0,
                event_groups: vec![EventGroup {
                    kind: EventGroupKind::Chord,
                    // B3 on string 2 then E4 on string 1: pitch-ascending, i.e.
                    // the reverse of the canonical string order.
                    atoms: vec![chord_note(0, 2, 0, 59), chord_note(0, 1, 0, 64)],
                    technique_spans: vec![],
                }],
            }],
            tuning: Tuning::standard_e(),
        }],
        source_meta: None,
        loss: LossReport::new(),
    };

    let normalized = normalize(&score);
    let strings: Vec<Option<u8>> = normalized.tracks[0].bars[0].voices[0]
        .notes
        .iter()
        .map(|note| note.string)
        .collect();
    assert_eq!(
        strings,
        vec![Some(1), Some(2)],
        "same-onset chord notes must order by string (ADR-0020), not pitch"
    );
}
