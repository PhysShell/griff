//! Shared fixture: one source, one fully populated corpus population, one ask.
#![allow(dead_code, clippy::expect_used)]

use griff_core::event::{NoteMarks, Pitch, Tempo, Ticks, TimeSignature, Tuning, Velocity};
use griff_core::generate::{RhythmTemplate, TemplateNote};
use griff_core::generation_input::{CorpusMaterial, GenerationAsk};
use griff_core::gesture::GestureControl;
use griff_core::score::{
    index_from_ordinal, AtomEvent, AtomNote, EventGroup, EventGroupKind, LossReport, MasterBar,
    RepeatMarker, Score, Track, Voice,
};
use griff_core::slice::TickRange;
use griff_experiment::{EvaluationContext, ExperimentSpec, InformationRegime, VariantSpec};

pub const BAR: u32 = 1920;

/// Two 4/4 bars of the given `(onset, duration, pitch)` notes on one track.
pub fn score_of(notes: &[(u32, u32, u8)]) -> Score {
    let master_bars = (0..2_usize)
        .map(|i| {
            let start = u32::try_from(i).expect("two bars").saturating_mul(BAR);
            MasterBar {
                index: index_from_ordinal(i),
                tick_range: TickRange::new(Ticks(start), Ticks(start.saturating_add(BAR)))
                    .expect("ordered"),
                time_signature: TimeSignature::new(4, 4).expect("4/4"),
                tempo: Tempo::from_bpm_integer(120).expect("120 BPM"),
                repeat: RepeatMarker::default(),
            }
        })
        .collect();
    let event_groups = notes
        .iter()
        .map(|&(onset, duration, pitch)| EventGroup {
            kind: EventGroupKind::Single,
            atoms: vec![AtomEvent::Note(AtomNote {
                absolute_start: Ticks(onset),
                duration: Ticks(duration),
                pitch: Pitch::new(pitch).expect("valid pitch"),
                velocity: Velocity::new(96).expect("valid velocity"),
                marks: NoteMarks::empty(),
                position: None,
            })],
            technique_spans: Vec::new(),
        })
        .collect();
    Score {
        ticks_per_quarter: 480,
        master_bars,
        tracks: vec![Track {
            name: Some("guitar".to_owned()),
            channel: 0,
            voices: vec![Voice {
                id: 0,
                event_groups,
            }],
            tuning: Tuning::standard_e(),
        }],
        source_meta: None,
        loss: LossReport::new(),
    }
}

pub fn source() -> Score {
    score_of(&[
        (0, 480, 40),
        (480, 480, 43),
        (960, 480, 45),
        (1440, 480, 47),
        (1920, 960, 50),
        (2880, 480, 47),
        (3360, 480, 45),
    ])
}

pub fn template(notes: &[(u32, u32)]) -> RhythmTemplate {
    RhythmTemplate {
        notes: notes
            .iter()
            .map(|&(offset, duration)| TemplateNote {
                offset: Ticks(offset),
                duration: Ticks(duration),
            })
            .collect(),
    }
}

/// A population with every channel populated.
pub fn corpus() -> CorpusMaterial {
    CorpusMaterial {
        rhythms: vec![
            template(&[(0, 240), (240, 240), (480, 480), (960, 960)]),
            template(&[(0, 480), (720, 240), (960, 240), (1440, 480)]),
        ],
        references: vec![
            score_of(&[(0, 480, 40), (480, 480, 43), (960, 480, 45)]),
            score_of(&[(0, 240, 52), (240, 240, 50), (480, 960, 47)]),
        ],
        gesture: Some(GestureControl {
            burst_notes: 3,
            rest_quarters: 1.0,
        }),
        skipped: vec!["unreadable.chunk.json".to_owned()],
    }
}

/// The same population with one reference changed, every other channel equal.
pub fn corpus_with_other_references() -> CorpusMaterial {
    let mut material = corpus();
    material.references[1] = score_of(&[(0, 240, 53), (240, 240, 50), (480, 960, 47)]);
    material
}

pub const fn ask() -> GenerationAsk {
    GenerationAsk {
        seed: 42,
        bars: 4,
        variants_per_strategy: 2,
        gesture: true,
        tonal: None,
    }
}

/// Milestone 1: S6 Intact and S7 Global Chain × seed-only and full.
pub fn two_by_two(evaluation: EvaluationContext) -> ExperimentSpec {
    ExperimentSpec {
        ask: ask(),
        variants: vec![VariantSpec::s6_intact(), VariantSpec::s7_global_chain()],
        regimes: vec![InformationRegime::SEED_ONLY, InformationRegime::FULL],
        evaluation,
    }
}
