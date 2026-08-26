//! SWG-4A-02: what [`v2::ExactScoreDocument`] must be able to hold.
//!
//! These witnesses are about **representation**, not acceptance. The exact
//! score text sits between a parser that does not exist yet (4A-06..08) and a
//! checked builder that does not exist yet (4A-09):
//!
//! ```text
//! level-2 text  ->  ExactScoreDocument  ->  ScoreBuilder  ->  griff_core::Score
//! ```
//!
//! This task builds only the middle form. Its whole job is to be able to say
//! what the grammar can spell — including things the parser will reject and
//! things the builder will refuse — so that neither of those later tasks
//! finds its work already done, badly, by a struct definition.
//!
//! So a document that holds `ppqn 0` is not a claim that `ppqn 0` will ever
//! be accepted. It is a claim that the *syntax type* did not quietly appoint
//! itself the validator.

use super::v2::{
    ExactAtom, ExactEvidence, ExactEvidenceSource, ExactGroup, ExactGroupKind, ExactMasterBar,
    ExactMeter, ExactNote, ExactNoteMark, ExactPosition, ExactRepeat, ExactRest,
    ExactScoreDocument, ExactSource, ExactSpan, ExactSpanTechnique, ExactTempo, ExactTickRange,
    ExactTrack, ExactVoice, ExactWarning,
};

// ── a whole tree, once ──────────────────────────────────────────────────────

/// The §6.1 reference document as data: every level of the grammar tree, so
/// that a missing child type is a compile error rather than a discovery made
/// later by the parser.
fn reference() -> ExactScoreDocument {
    ExactScoreDocument {
        ppqn: 960,
        master_bars: reference_bars(),
        tracks: vec![reference_track()],
        source: Some(ExactSource {
            format: Some(String::from("GP5")),
        }),
        loss: vec![ExactWarning::TempoApproximated {
            bar_index: 1,
            nearest_micros: 4_200_000,
        }],
    }
}

fn reference_bars() -> Vec<ExactMasterBar> {
    vec![
        ExactMasterBar {
            index: 0,
            ticks: ExactTickRange {
                start: 0,
                end: 3840,
            },
            meter: ExactMeter {
                numerator: 4,
                denominator: 4,
            },
            tempo: ExactTempo {
                numerator: 120,
                denominator: 1,
            },
            repeat: None,
        },
        ExactMasterBar {
            index: 1,
            ticks: ExactTickRange {
                start: 3840,
                end: 7680,
            },
            meter: ExactMeter {
                numerator: 7,
                denominator: 8,
            },
            tempo: ExactTempo {
                numerator: 100,
                denominator: 7,
            },
            repeat: Some(ExactRepeat {
                start: true,
                play_count: 2,
            }),
        },
    ]
}

fn reference_track() -> ExactTrack {
    ExactTrack {
        name: Some(String::from("Guitar")),
        channel: 0,
        tuning: vec![64, 59, 55, 50, 45, 40],
        voices: vec![ExactVoice {
            id: 0,
            groups: vec![reference_chord(), reference_rest(), reference_tuplet()],
        }],
    }
}

fn reference_chord() -> ExactGroup {
    ExactGroup {
        kind: ExactGroupKind::Chord,
        atoms: vec![
            ExactAtom::Note(ExactNote {
                at: 0,
                duration: 480,
                pitch: 40,
                velocity: 96,
                marks: Vec::new(),
                position: None,
            }),
            ExactAtom::Note(ExactNote {
                at: 0,
                duration: 480,
                pitch: 47,
                velocity: 96,
                marks: vec![ExactNoteMark::Accent],
                position: None,
            }),
        ],
        spans: vec![ExactSpan {
            technique: ExactSpanTechnique::PalmMute,
            ticks: ExactTickRange { start: 0, end: 480 },
            evidence: ExactEvidence {
                source: ExactEvidenceSource::Explicit,
                confidence: 10_000,
            },
        }],
    }
}

fn reference_rest() -> ExactGroup {
    ExactGroup {
        kind: ExactGroupKind::Single,
        atoms: vec![ExactAtom::Rest(ExactRest {
            at: 480,
            duration: 240,
        })],
        spans: Vec::new(),
    }
}

fn reference_tuplet() -> ExactGroup {
    ExactGroup {
        kind: ExactGroupKind::Tuplet { num: 3, den: 2 },
        atoms: vec![ExactAtom::Note(ExactNote {
            at: 720,
            duration: 160,
            pitch: 52,
            velocity: 80,
            marks: Vec::new(),
            position: Some(ExactPosition {
                string: 4,
                fret: 2,
                evidence: ExactEvidence {
                    source: ExactEvidenceSource::InferredFromMidi,
                    confidence: 5_000,
                },
            }),
        })],
        spans: Vec::new(),
    }
}

#[test]
fn the_whole_grammar_tree_is_representable() {
    let document = reference();
    assert_eq!(document, reference(), "the form is plain, comparable data");
    assert_eq!(document.master_bars.len(), 2);
    assert_eq!(document.tracks.len(), 1);
}

// ── one repeated slot, not two collections ──────────────────────────────────

#[test]
fn notes_and_rests_share_one_ordered_slot() {
    // §6.2 rule 3: `note` and `rest` are variant tags at one position of a
    // single sequence. Two collections would make the interleaving
    // unrepresentable, and re-ordering it would rewrite the music.
    let group = ExactGroup {
        kind: ExactGroupKind::Single,
        atoms: vec![
            ExactAtom::Note(ExactNote {
                at: 0,
                duration: 240,
                pitch: 40,
                velocity: 90,
                marks: Vec::new(),
                position: None,
            }),
            ExactAtom::Rest(ExactRest {
                at: 240,
                duration: 240,
            }),
            ExactAtom::Note(ExactNote {
                at: 480,
                duration: 240,
                pitch: 43,
                velocity: 90,
                marks: Vec::new(),
                position: None,
            }),
        ],
        spans: Vec::new(),
    };
    let kinds: Vec<bool> = group
        .atoms
        .iter()
        .map(|atom| matches!(atom, ExactAtom::Note(_)))
        .collect();
    assert_eq!(
        kinds,
        vec![true, false, true],
        "note, rest, note stays one ordered vector"
    );
}

#[test]
fn all_four_warning_variants_share_one_ordered_slot() {
    // Order is vector order and duplicates are duplicates (§2.7): A, B, A is
    // three elements, in that sequence.
    let loss = vec![
        ExactWarning::SmpteTimingUnsupported,
        ExactWarning::Other(String::from("something else")),
        ExactWarning::SmpteTimingUnsupported,
        ExactWarning::TrackNameInvalidUtf8 { track_index: 2 },
    ];
    let document = ExactScoreDocument {
        loss: loss.clone(),
        ..reference()
    };
    assert_eq!(document.loss, loss, "no sorting, no grouping, no dedup");
    assert_eq!(
        document.loss.first(),
        document.loss.get(2),
        "a repeated warning is repeated, not collapsed"
    );
}

#[test]
fn marks_keep_what_the_text_spelled() {
    // The canonical model's `NoteMarks` is a set whose order is
    // `NoteMark::ALL`. That is the *model's* rule, discharged by 4A-09. A
    // sequence here is what lets the document hold an order or a repetition
    // the builder will later refuse, instead of normalising it away at the
    // syntax boundary and hiding whose refusal it was.
    let note = ExactNote {
        at: 0,
        duration: 480,
        pitch: 40,
        velocity: 90,
        marks: vec![
            ExactNoteMark::Tap,
            ExactNoteMark::Accent,
            ExactNoteMark::Tap,
        ],
        position: None,
    };
    assert_eq!(note.marks.len(), 3, "no dedup");
    assert_eq!(
        note.marks.first(),
        Some(&ExactNoteMark::Tap),
        "no reordering into NoteMark::ALL order"
    );
}

// ── absence is a distinction ────────────────────────────────────────────────

#[test]
fn the_three_source_states_are_three_values() {
    let absent = ExactScoreDocument {
        source: None,
        ..reference()
    };
    let present_empty = ExactScoreDocument {
        source: Some(ExactSource { format: None }),
        ..reference()
    };
    let empty_format = ExactScoreDocument {
        source: Some(ExactSource {
            format: Some(String::new()),
        }),
        ..reference()
    };
    assert_ne!(absent, present_empty, "omitted is not present-and-empty");
    assert_ne!(present_empty, empty_format, "no format is not an empty one");
    assert_ne!(absent, empty_format);
}

#[test]
fn an_unnamed_track_is_not_an_empty_named_one() {
    let unnamed = ExactTrack {
        name: None,
        channel: 0,
        tuning: Vec::new(),
        voices: Vec::new(),
    };
    let empty_name = ExactTrack {
        name: Some(String::new()),
        ..unnamed.clone()
    };
    assert_ne!(unnamed, empty_name);
}

/// A bar with no `repeat` block at all.
fn bar_without_repeat() -> ExactMasterBar {
    ExactMasterBar {
        index: 0,
        ticks: ExactTickRange { start: 0, end: 0 },
        meter: ExactMeter {
            numerator: 4,
            denominator: 4,
        },
        tempo: ExactTempo {
            numerator: 120,
            denominator: 1,
        },
        repeat: None,
    }
}

#[test]
fn an_absent_repeat_is_not_a_default_one() {
    let written = ExactMasterBar {
        repeat: Some(ExactRepeat {
            start: false,
            play_count: 0,
        }),
        ..bar_without_repeat()
    };
    assert_ne!(
        bar_without_repeat(),
        written,
        "a repeat block the text omitted is not one it spelled as the default"
    );
}

// ── every closed vocabulary is complete ─────────────────────────────────────

#[test]
fn every_group_kind_is_representable() {
    let kinds = [
        ExactGroupKind::Single,
        ExactGroupKind::Chord,
        ExactGroupKind::Arpeggio,
        ExactGroupKind::Strum,
        ExactGroupKind::Tuplet { num: 3, den: 2 },
        ExactGroupKind::Grace,
    ];
    assert_eq!(kinds.len(), 6, "§6.4's six group kinds");
}

#[test]
fn every_span_technique_is_representable() {
    let techniques = [
        ExactSpanTechnique::Slide,
        ExactSpanTechnique::Bend,
        ExactSpanTechnique::Legato,
        ExactSpanTechnique::PalmMute,
        ExactSpanTechnique::HammerOn,
        ExactSpanTechnique::PullOff,
        ExactSpanTechnique::Vibrato,
        ExactSpanTechnique::LetRing,
    ];
    assert_eq!(techniques.len(), 8, "§6.4's eight span techniques");
}

#[test]
fn every_note_mark_and_evidence_source_is_representable() {
    let marks = [
        ExactNoteMark::Accent,
        ExactNoteMark::Ghost,
        ExactNoteMark::Staccato,
        ExactNoteMark::DeadNote,
        ExactNoteMark::HarmonicNatural,
        ExactNoteMark::HarmonicPinch,
        ExactNoteMark::Tap,
    ];
    assert_eq!(marks.len(), 7, "§6.3's seven marks");
    let sources = [
        ExactEvidenceSource::Explicit,
        ExactEvidenceSource::InferredFromMidi,
    ];
    assert_eq!(sources.len(), 2);
}

#[test]
fn every_warning_variant_carries_its_own_payload() {
    let warnings = [
        ExactWarning::TrackNameInvalidUtf8 { track_index: 0 },
        ExactWarning::SmpteTimingUnsupported,
        ExactWarning::TempoApproximated {
            bar_index: 0,
            nearest_micros: 500_000,
        },
        ExactWarning::Other(String::new()),
    ];
    assert_eq!(warnings.len(), 4, "§2.8's four warning variants");
}

// ── ugly, legal, canonical states ───────────────────────────────────────────

#[test]
fn the_states_the_model_calls_inhabited_are_representable() {
    // §3 lists all of these as model-valid. A syntax form that could not
    // hold them would make the writer's own output unrepresentable.
    let document = ExactScoreDocument {
        tracks: vec![ExactTrack {
            name: None,
            channel: 200,
            tuning: Vec::new(),
            voices: vec![
                ExactVoice {
                    id: 0,
                    groups: vec![ExactGroup {
                        kind: ExactGroupKind::Tuplet { num: 0, den: 0 },
                        atoms: vec![
                            ExactAtom::Note(ExactNote {
                                at: 0,
                                duration: 0,
                                pitch: 40,
                                velocity: 90,
                                marks: Vec::new(),
                                position: Some(ExactPosition {
                                    string: 99,
                                    fret: 250,
                                    evidence: ExactEvidence {
                                        source: ExactEvidenceSource::Explicit,
                                        confidence: 0,
                                    },
                                }),
                            }),
                            ExactAtom::Rest(ExactRest { at: 0, duration: 0 }),
                        ],
                        spans: Vec::new(),
                    }],
                },
                ExactVoice {
                    id: 0,
                    groups: Vec::new(),
                },
            ],
        }],
        ..reference()
    };
    let voices = &document.tracks.first().expect("one track").voices;
    assert_eq!(voices.len(), 2, "duplicate voice ids are not deduplicated");
    assert_eq!(
        voices.first().map(|v| v.id),
        voices.get(1).map(|v| v.id),
        "and they really are the same id"
    );
}

#[test]
fn the_states_a_later_task_will_refuse_are_still_representable() {
    // None of this is a claim that such a text will be accepted. It is the
    // claim that 4A-02 did not decide — the scalar parser (4A-07) and the
    // checked builder (4A-09) each still have their refusals to make, and a
    // struct that could not hold the value would have made those refusals
    // unreachable and unattributable.
    let document = ExactScoreDocument {
        ppqn: 0,
        master_bars: vec![ExactMasterBar {
            index: 0,
            ticks: ExactTickRange {
                start: 100,
                end: 10,
            },
            meter: ExactMeter {
                numerator: 0,
                denominator: 3,
            },
            tempo: ExactTempo {
                numerator: 0,
                denominator: 0,
            },
            repeat: None,
        }],
        tracks: vec![ExactTrack {
            name: None,
            channel: 0,
            tuning: vec![200],
            voices: vec![ExactVoice {
                id: 0,
                groups: vec![ExactGroup {
                    kind: ExactGroupKind::Single,
                    atoms: vec![ExactAtom::Note(ExactNote {
                        at: 0,
                        duration: 1,
                        pitch: 200,
                        velocity: 255,
                        marks: Vec::new(),
                        position: None,
                    })],
                    spans: vec![ExactSpan {
                        technique: ExactSpanTechnique::Bend,
                        ticks: ExactTickRange {
                            start: 500,
                            end: 100,
                        },
                        evidence: ExactEvidence {
                            source: ExactEvidenceSource::InferredFromMidi,
                            confidence: 65_535,
                        },
                    }],
                }],
            }],
        }],
        source: None,
        loss: Vec::new(),
    };
    assert_eq!(document.ppqn, 0, "no constructor refused it");
    assert_eq!(
        document.master_bars.first().map(|b| b.ticks.start),
        Some(100),
        "an inverted range is stored as written"
    );
}

// ── widths ──────────────────────────────────────────────────────────────────

#[test]
fn both_canonical_indices_are_wide_enough_for_the_values_core_admits() {
    // SWG-CORE-01 closed H3 at `u64` because values above `u32::MAX` were
    // already inhabited. A syntax form narrower than the model would lose
    // them on the way in, which is the same defect one layer up.
    let big = u64::from(u32::MAX) + 1;
    let document = ExactScoreDocument {
        master_bars: vec![ExactMasterBar {
            index: big,
            ticks: ExactTickRange { start: 0, end: 1 },
            meter: ExactMeter {
                numerator: 4,
                denominator: 4,
            },
            tempo: ExactTempo {
                numerator: 120,
                denominator: 1,
            },
            repeat: None,
        }],
        loss: vec![
            ExactWarning::TrackNameInvalidUtf8 { track_index: big },
            ExactWarning::TempoApproximated {
                bar_index: big,
                nearest_micros: u32::MAX,
            },
        ],
        ..reference()
    };
    assert_eq!(document.master_bars.first().map(|b| b.index), Some(big));
    assert_eq!(
        document.loss.first(),
        Some(&ExactWarning::TrackNameInvalidUtf8 { track_index: big }),
        "the warning payload survives past u32::MAX too"
    );
}

// ── strings ─────────────────────────────────────────────────────────────────

#[test]
fn strings_are_arbitrary_utf8_not_level_one_string_literals() {
    // Level 1's `StringLiteral` refuses quotes and line breaks by
    // construction, because its grammar has no escapes. §6.5 gives level 2
    // an escape policy, so the *value* side must hold anything a `String`
    // can — that is what the escapes exist to spell.
    let awkward = "a \" b \\ c \n d \r e \u{85} f \u{1f} g é 音";
    let document = ExactScoreDocument {
        tracks: vec![ExactTrack {
            name: Some(String::from(awkward)),
            channel: 0,
            tuning: Vec::new(),
            voices: Vec::new(),
        }],
        source: Some(ExactSource {
            format: Some(String::from(awkward)),
        }),
        loss: vec![ExactWarning::Other(String::from(awkward))],
        ..reference()
    };
    assert_eq!(
        document.tracks.first().and_then(|t| t.name.as_deref()),
        Some(awkward),
        "the value is the value; escaping belongs to the text, not the tree"
    );
    assert_eq!(
        document.loss.first(),
        Some(&ExactWarning::Other(String::from(awkward)))
    );
}
