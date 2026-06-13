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
        AtomEvent, AtomNote, EventGroup, EventGroupKind, MasterBar, Score, TechniqueSpan, Track,
        Voice,
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
        loss: griff_core::score::LossReport::new(),
    }
}

#[test]
fn normalized_dump_golden() {
    let normalized = normalize(&sample_score());
    let json = serde_json::to_string_pretty(&normalized).expect("serialize");
    insta::assert_snapshot!(json);
}
