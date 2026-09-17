// TDD red: the canonical semantic projection V1 (ADR-0034 decision 5) — one
// projection of every recorded model value, lossless both ways on everything
// the model can hold, typed refusal of anything it cannot, and the single
// source every fingerprint walks.
#![allow(
    clippy::expect_used,
    clippy::missing_assert_message,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::arithmetic_side_effects
)]

mod common;

use common::{ask, corpus, source};
use griff_core::event::{
    ConfidenceBps, FretboardPosition, NoteMark, NoteMarks, NotePosition, Pitch, SpanTechnique,
    TechniqueEvidence, Tempo, Ticks, TimeSignature, Tuning, Velocity,
};
use griff_core::generation_input::{generation_request_from_score, GenerationAsk};
use griff_core::score::{
    index_from_ordinal, AtomEvent, AtomNote, AtomRest, EventGroup, EventGroupKind, ImportWarning,
    LossReport, MasterBar, RepeatMarker, Score, SourceMeta, TechniqueSpan, Track, Voice,
};
use griff_core::slice::TickRange;
use griff_core::tonal::{EvidenceScope, TonalContext};
use griff_experiment::{
    ask_fingerprint, gesture_fingerprint, rhythms_fingerprint, score_fingerprint, AtomV1,
    EvidenceV1, GenerationAskV1, GestureControlV1, NotePositionV1, NoteV1, PitchMaterialV1,
    ProjectionError, RhythmTemplateV1, ScoreV1, TechniqueSourceV1,
};

fn note(start: u32, pitch: u8, marks: NoteMarks, position: Option<NotePosition>) -> AtomEvent {
    AtomEvent::Note(AtomNote {
        absolute_start: Ticks(start),
        duration: Ticks(240),
        pitch: Pitch::new(pitch).expect("pitch"),
        velocity: Velocity::new(127).expect("velocity"),
        marks,
        position,
    })
}

fn bar(index: usize, start: u32, tempo: Tempo, repeat: RepeatMarker) -> MasterBar {
    MasterBar {
        index: index_from_ordinal(index),
        tick_range: TickRange::new(Ticks(start), Ticks(start + 1440)).expect("ordered"),
        time_signature: TimeSignature::new(3, 4).expect("3/4"),
        tempo,
        repeat,
    }
}

/// A score exercising every field and every variant the model has.
fn rich_score() -> Score {
    let explicit = TechniqueEvidence::explicit();
    let inferred = TechniqueEvidence::inferred(ConfidenceBps::new(7_250).expect("bps"));
    let all_marks = NoteMark::ALL
        .iter()
        .fold(NoteMarks::empty(), |marks, &mark| marks.with(mark));
    let kinds = [
        EventGroupKind::Single,
        EventGroupKind::Chord,
        EventGroupKind::Arpeggio,
        EventGroupKind::Strum,
        EventGroupKind::Tuplet { num: 3, den: 2 },
        EventGroupKind::Grace,
    ];
    let techniques = [
        SpanTechnique::Slide,
        SpanTechnique::Bend,
        SpanTechnique::Legato,
        SpanTechnique::PalmMute,
        SpanTechnique::HammerOn,
        SpanTechnique::PullOff,
        SpanTechnique::Vibrato,
        SpanTechnique::LetRing,
    ];
    let event_groups = kinds
        .iter()
        .enumerate()
        .map(|(i, &kind)| {
            let start = u32::try_from(i).expect("small") * 240;
            EventGroup {
                kind,
                atoms: vec![
                    note(
                        start,
                        40 + u8::try_from(i).expect("small"),
                        if i % 2 == 0 {
                            all_marks
                        } else {
                            NoteMarks::empty().with(NoteMark::Tap)
                        },
                        Some(NotePosition {
                            position: FretboardPosition {
                                string: 6,
                                fret: u8::try_from(i).expect("small"),
                            },
                            evidence: if i % 2 == 0 { explicit } else { inferred },
                        }),
                    ),
                    note(start, 52, NoteMarks::empty(), None),
                    AtomEvent::Rest(AtomRest {
                        absolute_start: Ticks(start + 120),
                        duration: Ticks(120),
                    }),
                ],
                technique_spans: techniques
                    .iter()
                    .take(i + 2)
                    .map(|&technique| TechniqueSpan {
                        technique,
                        tick_range: TickRange::new(Ticks(start), Ticks(start + 240))
                            .expect("ordered"),
                        evidence: inferred,
                    })
                    .collect(),
            }
        })
        .collect();
    Score {
        ticks_per_quarter: 960,
        master_bars: vec![
            bar(
                0,
                0,
                Tempo::from_micros_per_quarter(428_571).expect("tempo"),
                RepeatMarker {
                    start: true,
                    play_count: 0,
                },
            ),
            bar(
                1,
                1440,
                Tempo::from_bpm_integer(173).expect("tempo"),
                RepeatMarker {
                    start: false,
                    play_count: 3,
                },
            ),
        ],
        tracks: vec![
            Track {
                name: Some("Guitar — drop C#".to_owned()),
                channel: 9,
                voices: vec![
                    Voice {
                        id: 0,
                        event_groups,
                    },
                    Voice {
                        id: 1,
                        event_groups: Vec::new(),
                    },
                ],
                tuning: Tuning::new(
                    [61, 56, 52, 47, 42, 37]
                        .into_iter()
                        .map(|p| Pitch::new(p).expect("pitch"))
                        .collect(),
                ),
            },
            Track {
                name: None,
                channel: 0,
                voices: Vec::new(),
                tuning: Tuning::standard_e(),
            },
        ],
        source_meta: Some(SourceMeta {
            format: Some("gp5".to_owned()),
        }),
        loss: LossReport {
            warnings: vec![
                ImportWarning::TrackNameInvalidUtf8 { track_index: 1 },
                ImportWarning::SmpteTimingUnsupported,
                ImportWarning::TempoApproximated {
                    bar_index: 4,
                    nearest_micros: 428_571,
                },
                ImportWarning::Other("tuplet 7:5 approximated".to_owned()),
            ],
        },
    }
}

// ── lossless on everything the model holds ───────────────────────────────────

#[test]
fn every_score_field_survives_the_projection() {
    let score = rich_score();
    let projected = ScoreV1::from(&score);
    assert_eq!(projected.to_score(), Ok(score.clone()));
    let json = serde_json::to_string(&projected).expect("serializes");
    let back: ScoreV1 = serde_json::from_str(&json).expect("deserializes");
    assert_eq!(back, projected, "the wire form is the projection, verbatim");
    assert_eq!(back.to_score(), Ok(score));
}

#[test]
fn every_constructible_tempo_survives_the_projection() {
    let tempos = [1_u32, 7, 120, 173, 999_999, u32::MAX]
        .map(|bpm| Tempo::from_bpm_integer(bpm).expect("bpm"))
        .into_iter()
        .chain(
            [1_u32, 7, 428_571, 500_000, 60_000_000, 60_000_001, u32::MAX]
                .map(|micros| Tempo::from_micros_per_quarter(micros).expect("micros")),
        );
    for tempo in tempos {
        let mut score = source();
        score.master_bars[0].tempo = tempo;
        assert_eq!(
            ScoreV1::from(&score).to_score(),
            Ok(score),
            "{}/{} BPM",
            tempo.bpm_numerator(),
            tempo.bpm_denominator()
        );
    }
}

#[test]
fn the_generation_inputs_survive_the_projection() {
    let c = corpus();
    for template in &c.rhythms {
        assert_eq!(&RhythmTemplateV1::from(template).to_template(), template);
    }
    let gesture = c.gesture.expect("the fixture carries one");
    assert_eq!(GestureControlV1::from(gesture).to_gesture(), Ok(gesture));

    let material = generation_request_from_score(&source(), 42, 4)
        .expect("seeds")
        .pitch_material;
    let back = PitchMaterialV1::from(&material)
        .to_material()
        .expect("valid");
    assert_eq!(
        (back.root, back.intervals),
        (material.root, material.intervals)
    );

    for tonal in [
        None,
        Some(TonalContext::measure(&source(), EvidenceScope::WholeScore)),
    ] {
        let asked = GenerationAsk { tonal, ..ask() };
        let back = GenerationAskV1::from(&asked).to_ask().expect("valid");
        assert_eq!(
            (
                back.seed,
                back.bars,
                back.variants_per_strategy,
                back.gesture,
                back.tonal
            ),
            (
                asked.seed,
                asked.bars,
                asked.variants_per_strategy,
                asked.gesture,
                asked.tonal
            )
        );
    }
}

// ── refused, typed, never coerced ────────────────────────────────────────────

/// The fixture's first note, projected.
fn first_note(p: &mut ScoreV1) -> &mut NoteV1 {
    match &mut p.tracks[0].voices[0].event_groups[0].atoms[0] {
        AtomV1::Note(n) => n,
        AtomV1::Rest(_) => panic!("the fixture opens on a note"),
    }
}

#[test]
fn a_projection_the_model_cannot_hold_is_refused() {
    let valid = ScoreV1::from(&source());
    let refused = |edit: &dyn Fn(&mut ScoreV1)| {
        let mut projected = valid.clone();
        edit(&mut projected);
        projected.to_score().expect_err("refused")
    };

    assert_eq!(
        refused(&|p| first_note(p).pitch = 128),
        ProjectionError::InvalidPitch(128)
    );
    assert_eq!(
        refused(&|p| first_note(p).velocity = 200),
        ProjectionError::InvalidVelocity(200)
    );
    assert_eq!(
        refused(&|p| p.tracks[0].tuning[0] = 128),
        ProjectionError::InvalidPitch(128)
    );
    assert_eq!(
        refused(&|p| p.master_bars[0].tick_range.start = 5_000),
        ProjectionError::InvalidTickRange {
            start: 5_000,
            end: 1920
        }
    );
    assert_eq!(
        refused(&|p| p.master_bars[0].denominator = 3),
        ProjectionError::InvalidTimeSignature {
            numerator: 4,
            denominator: 3
        }
    );
    assert_eq!(
        refused(&|p| {
            p.master_bars[0].bpm_numerator = 7;
            p.master_bars[0].bpm_denominator = 3;
        }),
        ProjectionError::UnrepresentableTempo {
            numerator: 7,
            denominator: 3
        },
        "no model constructor reduces to 7/3 BPM"
    );
    assert_eq!(
        refused(&|p| p.master_bars[0].bpm_numerator = 0),
        ProjectionError::UnrepresentableTempo {
            numerator: 0,
            denominator: 1
        }
    );
    assert_eq!(
        refused(&|p| {
            first_note(p).position = Some(NotePositionV1 {
                string: 1,
                fret: 0,
                evidence: EvidenceV1 {
                    source: TechniqueSourceV1::Explicit,
                    confidence_bps: 10_001,
                },
            });
        }),
        ProjectionError::InvalidConfidence(10_001)
    );
    assert_eq!(
        GestureControlV1 {
            burst_notes: 3,
            rest_quarters: f64::NAN
        }
        .to_gesture(),
        Err(ProjectionError::NonFiniteGesture)
    );
}

#[test]
fn an_unknown_field_is_refused_not_dropped() {
    let mut json: serde_json::Value =
        serde_json::to_value(ScoreV1::from(&source())).expect("serializes");
    json["tracks"][0]["capo"] = serde_json::json!(2);
    assert!(serde_json::from_value::<ScoreV1>(json).is_err());
}

// ── the one canonicalization ─────────────────────────────────────────────────

#[test]
fn every_fingerprint_is_the_walk_of_the_projection() {
    let score = rich_score();
    assert_eq!(
        score_fingerprint(&score),
        ScoreV1::from(&score).fingerprint()
    );
    let c = corpus();
    assert_eq!(
        rhythms_fingerprint(&c.rhythms),
        RhythmTemplateV1::fingerprint_all(
            &c.rhythms
                .iter()
                .map(RhythmTemplateV1::from)
                .collect::<Vec<_>>()
        )
    );
    assert_eq!(
        gesture_fingerprint(c.gesture),
        GestureControlV1::fingerprint_option(c.gesture.map(GestureControlV1::from))
    );
    assert_eq!(
        ask_fingerprint(&ask()),
        GenerationAskV1::from(&ask()).fingerprint()
    );
}
