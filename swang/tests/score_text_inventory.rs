//! Model-inventory characterization for the exact canonical score text
//! (S16 Phase 4A0; spec §4).
//!
//! Spec §4.4/§4.7 assign exactly one canonical textual form to every
//! canonical `Score` fact. These tests make that assignment a
//! compile-time contract before any writer or parser exists:
//!
//! - every struct of the canonical tree is destructured exhaustively (no
//!   `..`), with each binding annotated by its assigned keyword — a new
//!   model field breaks this file's compilation, forcing a spec update in
//!   the same change;
//! - every enum is matched exhaustively against its assigned literal — a
//!   new variant does the same;
//! - representation decisions that reuse existing exact-scalar APIs
//!   (rational tempo forms, confidence bps, `NoteMark::ALL` order) are
//!   pinned as data.
//!
//! Characterization of existing model facts: no new API, no production
//! change, green before commit.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::missing_assert_message,
    clippy::indexing_slicing,
    clippy::missing_const_for_fn
)]

use std::collections::HashSet;

use griff_core::{
    event::{
        ConfidenceBps, FretboardPosition, NoteMark, NoteMarks, NotePosition, Pitch, SpanTechnique,
        TechniqueEvidence, TechniqueSource, Tempo, Ticks, TimeSignature, Tuning, Velocity,
    },
    score::{
        AtomEvent, AtomNote, AtomRest, EventGroup, EventGroupKind, ImportWarning, LossReport,
        MasterBar, RepeatMarker, Score, SourceMeta, TechniqueSpan, Track, Voice,
    },
    slice::TickRange,
};

// ── a fully populated score: every optional present, every list non-empty ─────

fn full_score() -> Score {
    let tick_range =
        |start: u32, end: u32| TickRange::new(Ticks(start), Ticks(end)).expect("ordered range");
    Score {
        ticks_per_quarter: 480,
        master_bars: vec![MasterBar {
            index: 0,
            tick_range: tick_range(0, 1920),
            time_signature: TimeSignature::new(7, 8).expect("7/8"),
            tempo: Tempo::from_micros_per_quarter(495_868).expect("valid micros"),
            repeat: RepeatMarker {
                start: true,
                play_count: 2,
            },
        }],
        tracks: vec![Track {
            name: Some("lead".to_owned()),
            channel: 0,
            voices: vec![Voice {
                id: 0,
                event_groups: vec![EventGroup {
                    kind: EventGroupKind::Tuplet { num: 3, den: 2 },
                    atoms: vec![
                        AtomEvent::Note(AtomNote {
                            absolute_start: Ticks(0),
                            duration: Ticks(240),
                            pitch: Pitch::new(40).expect("valid"),
                            velocity: Velocity::new(96).expect("valid"),
                            marks: NoteMarks::empty().with(NoteMark::Accent),
                            position: Some(NotePosition::inferred(
                                FretboardPosition { string: 6, fret: 0 },
                                ConfidenceBps::HALF,
                            )),
                        }),
                        AtomEvent::Rest(AtomRest {
                            absolute_start: Ticks(240),
                            duration: Ticks(240),
                        }),
                    ],
                    technique_spans: vec![TechniqueSpan {
                        technique: SpanTechnique::PalmMute,
                        tick_range: tick_range(0, 480),
                        evidence: TechniqueEvidence::explicit(),
                    }],
                }],
            }],
            tuning: Tuning::standard_e(),
        }],
        source_meta: Some(SourceMeta {
            format: Some("GP5".to_owned()),
        }),
        loss: LossReport {
            warnings: vec![ImportWarning::Other("w0".to_owned())],
        },
    }
}

// ── struct inventory: one keyword per canonical field (spec §4.4) ─────────────

/// Exhaustively destructures the whole tree, annotating each binding with
/// its spec §4.4 keyword. A new canonical field fails this compilation and
/// forces the grammar contract to grow in the same change.
#[test]
fn every_canonical_field_has_an_assigned_textual_form() {
    let score = full_score();
    let Score {
        ticks_per_quarter: _ppq, // `ppq <u16>`
        master_bars,             // repeated `master_bar { … }`
        tracks,                  // repeated `track { … }`
        source_meta,             // `source_meta { … }`, omitted iff None
        loss,                    // `loss { warning … }`, omitted iff empty
    } = score;

    let MasterBar {
        index: _index,          // `index <usize>`
        tick_range: _range,     // `range <u32>..<u32>`
        time_signature: _meter, // `meter <num>/<den>`
        tempo: _tempo,          // `tempo <n>` | `tempo <num>/<den>`
        repeat,                 // `repeat { … }`, omitted iff default
    } = master_bars[0];
    let RepeatMarker {
        start: _start,           // `start <bool>`
        play_count: _play_count, // `play_count <u8>`
    } = repeat;

    let Track {
        name: _name,       // `name "<string>"`, omitted iff None
        channel: _channel, // `channel <u8>`
        voices,            // repeated `voice { … }`
        tuning: _tuning,   // `tuning [<u8> …]` (model order: last)
    } = tracks.into_iter().next().expect("one track");

    let Voice {
        id: _id,      // `id <u8>`
        event_groups, // repeated `group { … }`
    } = voices.into_iter().next().expect("one voice");

    let EventGroup {
        kind: _kind,     // `kind <variant>` (spec §4.7)
        atoms,           // repeated `note { … }` / `rest { … }`
        technique_spans, // repeated `span { … }`
    } = event_groups.into_iter().next().expect("one group");

    match atoms[0] {
        AtomEvent::Note(AtomNote {
            absolute_start: _note_start, // `start <u32>`
            duration: _note_duration,    // `duration <u32>`
            pitch: _pitch,               // `pitch <u8>`
            velocity: _velocity,         // `velocity <u8>`
            marks: _marks,               // `marks [<mark> …]`, omitted iff empty
            position,                    // `position { … }`, omitted iff None
        }) => {
            let NotePosition {
                position:
                    FretboardPosition {
                        string: _string, // `string <u8>`
                        fret: _fret,     // `fret <u8>`
                    },
                evidence,
            } = position.expect("the full score has a position");
            let TechniqueEvidence {
                source: _source,         // `evidence <source> …` (spec §4.7)
                confidence: _confidence, // `… <bps>` — always written
            } = evidence;
        }
        AtomEvent::Rest(AtomRest {
            absolute_start: _rest_start, // `start <u32>`
            duration: _rest_duration,    // `duration <u32>`
        }) => panic!("the fixture's first atom is a note"),
    }

    let TechniqueSpan {
        technique: _technique,    // `technique <variant>` (spec §4.7)
        tick_range: _span_range,  // `range <u32>..<u32>`
        evidence: _span_evidence, // `evidence <source> <bps>`
    } = technique_spans[0];

    let SourceMeta {
        format: _format, // `format "<string>"`, omitted iff None
    } = source_meta.expect("the full score has source meta");

    let LossReport {
        warnings: _warnings, // repeated `warning <variant> …`
    } = loss;
}

// ── enum inventories: one literal per variant (spec §4.7) ─────────────────────

fn group_kind_literal(kind: EventGroupKind) -> String {
    match kind {
        EventGroupKind::Single => "single".to_owned(),
        EventGroupKind::Chord => "chord".to_owned(),
        EventGroupKind::Arpeggio => "arpeggio".to_owned(),
        EventGroupKind::Strum => "strum".to_owned(),
        EventGroupKind::Tuplet { num, den } => format!("tuplet {{ num {num} den {den} }}"),
        EventGroupKind::Grace => "grace".to_owned(),
    }
}

fn mark_literal(mark: NoteMark) -> &'static str {
    match mark {
        NoteMark::Accent => "accent",
        NoteMark::Ghost => "ghost",
        NoteMark::Staccato => "staccato",
        NoteMark::DeadNote => "dead_note",
        NoteMark::HarmonicNatural => "harmonic_natural",
        NoteMark::HarmonicPinch => "harmonic_pinch",
        NoteMark::Tap => "tap",
    }
}

fn span_technique_literal(technique: SpanTechnique) -> &'static str {
    match technique {
        SpanTechnique::Slide => "slide",
        SpanTechnique::Bend => "bend",
        SpanTechnique::Legato => "legato",
        SpanTechnique::PalmMute => "palm_mute",
        SpanTechnique::HammerOn => "hammer_on",
        SpanTechnique::PullOff => "pull_off",
        SpanTechnique::Vibrato => "vibrato",
        SpanTechnique::LetRing => "let_ring",
    }
}

fn source_literal(source: TechniqueSource) -> &'static str {
    match source {
        TechniqueSource::Explicit => "explicit",
        TechniqueSource::InferredFromMidi => "inferred_from_midi",
    }
}

fn warning_form(warning: &ImportWarning) -> String {
    match warning {
        ImportWarning::TrackNameInvalidUtf8 { track_index } => {
            format!("warning track_name_invalid_utf8 {{ track_index {track_index} }}")
        }
        ImportWarning::SmpteTimingUnsupported => "warning smpte_timing_unsupported".to_owned(),
        ImportWarning::TempoApproximated {
            bar_index,
            nearest_micros,
        } => format!(
            "warning tempo_approximated {{ bar_index {bar_index} nearest_micros {nearest_micros} }}"
        ),
        ImportWarning::Other(message) => format!("warning other {message:?}"),
    }
}

#[test]
fn every_enum_variant_has_a_unique_snake_case_literal() {
    let mark_literals: Vec<&str> = NoteMark::ALL.into_iter().map(mark_literal).collect();
    let span_literals: Vec<&str> = [
        SpanTechnique::Slide,
        SpanTechnique::Bend,
        SpanTechnique::Legato,
        SpanTechnique::PalmMute,
        SpanTechnique::HammerOn,
        SpanTechnique::PullOff,
        SpanTechnique::Vibrato,
        SpanTechnique::LetRing,
    ]
    .into_iter()
    .map(span_technique_literal)
    .collect();

    for literal in mark_literals.iter().chain(span_literals.iter()).chain(
        [
            source_literal(TechniqueSource::Explicit),
            source_literal(TechniqueSource::InferredFromMidi),
        ]
        .iter(),
    ) {
        assert!(
            literal.chars().all(|c| c.is_ascii_lowercase() || c == '_'),
            "literals are snake_case: {literal}"
        );
    }
    let unique: HashSet<&&str> = mark_literals.iter().chain(span_literals.iter()).collect();
    assert_eq!(
        unique.len(),
        mark_literals.len() + span_literals.len(),
        "no two variants share a literal"
    );

    assert_eq!(
        group_kind_literal(EventGroupKind::Tuplet { num: 3, den: 2 }),
        "tuplet { num 3 den 2 }",
        "the one payload-carrying kind spells its fields (spec §4.7)"
    );
    assert_eq!(
        warning_form(&ImportWarning::TempoApproximated {
            bar_index: 0,
            nearest_micros: 495_868,
        }),
        "warning tempo_approximated { bar_index 0 nearest_micros 495868 }"
    );
}

// ── representation decisions pinned against existing exact-scalar APIs ────────

#[test]
fn tempo_textual_forms_follow_the_reduced_rational() {
    // Integer BPM prints without a denominator (spec §4.6).
    let integer = Tempo::from_bpm_integer(120).expect("120 BPM");
    assert_eq!(integer.bpm_denominator(), 1);

    // A MIDI-sourced tempo prints its reduced fraction, exactly as stored.
    let fraction = Tempo::from_micros_per_quarter(495_868).expect("valid micros");
    assert_eq!(
        (fraction.bpm_numerator(), fraction.bpm_denominator()),
        (15_000_000, 123_967),
        "the text carries the reduced pair the model already holds"
    );
}

#[test]
fn confidence_is_always_a_plain_bps_integer() {
    // Any (source, bps) pair is a valid model value, so the text always
    // spells the confidence — even the conventional ones (spec §4.6).
    assert_eq!(TechniqueEvidence::explicit().confidence.get(), 10_000);
    assert_eq!(ConfidenceBps::HALF.get(), 5_000);
}

#[test]
fn marks_render_in_note_mark_all_order() {
    // The canonical `marks [...]` order is `NoteMark::ALL` — the same order
    // `NoteMarks::iter` already yields, pinned here as the grammar's order.
    let mut set = NoteMarks::empty();
    set.insert(NoteMark::Tap);
    set.insert(NoteMark::Accent);
    let rendered: Vec<&str> = set.iter().map(mark_literal).collect();
    assert_eq!(
        rendered,
        vec!["accent", "tap"],
        "insertion order never leaks into the canonical text"
    );
}

#[test]
fn repeat_absence_value_is_the_default_marker() {
    // Spec §4.8: `repeat` is omitted iff equal to this exact value; the
    // parser reconstructs it. A declared absence value, not a hidden
    // default.
    assert_eq!(
        RepeatMarker::default(),
        RepeatMarker {
            start: false,
            play_count: 0,
        }
    );
}
