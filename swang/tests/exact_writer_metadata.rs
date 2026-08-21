//! SWG-4A-05 step A: the writer's musical leaves — marks, positions,
//! technique spans, and the evidence on both — committed failing before any
//! implementation.
//!
//! This is the last exact-writer slice. SWG-4A-04 wrote the structure and
//! refused everything below it by name; step A reds the musical half of what
//! it refused, step B the metadata half, and one GREEN answers both.
//!
//! Every expectation is a decision `docs/swang/exact-score-text.md` already
//! made. Three of them are the ones a writer gets wrong on its own:
//!
//! - `marks` is a **set**, so its canonical order is `NoteMark::ALL` and not
//!   the order bits were set, and not alphabetical either;
//! - a span's range and a position's string may sit outside anything the
//!   group or the tuning would suggest, and §3 lists both as inhabited;
//! - evidence is two independent facts. `Explicit` at 0 bps and
//!   `InferredFromMidi` at 10 000 both exist and are written literally
//!   (§2.6) — an importer's problem, never a licence for the text to tidy
//!   the model.
//!
//! A few tests here pass already: they pin bytes SWG-4A-03 and SWG-4A-04
//! produce and that this slice must not move. They are marked where they sit.

// The allowances the repository's other test files take.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::missing_assert_message
)]

use griff_core::event::{
    ConfidenceBps, FretboardPosition, NoteMark, NoteMarks, NotePosition, Pitch, SpanTechnique,
    TechniqueEvidence, TechniqueSource, Tempo, Ticks, TimeSignature, Tuning, ValidationError,
    Velocity,
};
use griff_core::score::{
    AtomEvent, AtomNote, AtomRest, EventGroup, EventGroupKind, LossReport, MasterBar, RepeatMarker,
    Score, TechniqueSpan, Track, Voice,
};
use griff_core::semantic_diff::exact_semantic_diff;
use griff_core::slice::TickRange;
use griff_swang::exact::{write_score, ExactWriteError};

// ── fixtures ───────────────────────────────────────────────────────────────

fn range(start: u32, end: u32) -> TickRange {
    TickRange::new(Ticks(start), Ticks(end)).expect("ordered range")
}

fn bar() -> MasterBar {
    MasterBar {
        index: 0,
        tick_range: range(0, 1920),
        time_signature: TimeSignature::new(4, 4).expect("4/4"),
        tempo: Tempo::from_bpm_integer(120).expect("120 BPM"),
        repeat: RepeatMarker::default(),
    }
}

fn note(at: u32, duration: u32, pitch: u8, velocity: u8) -> AtomNote {
    AtomNote {
        absolute_start: Ticks(at),
        duration: Ticks(duration),
        pitch: Pitch::new(pitch).expect("a MIDI pitch"),
        velocity: Velocity::new(velocity).expect("a MIDI velocity"),
        marks: NoteMarks::empty(),
        position: None,
    }
}

fn marks_of(set: &[NoteMark]) -> NoteMarks {
    set.iter()
        .fold(NoteMarks::empty(), |acc, &mark| acc.with(mark))
}

fn evidence(source: TechniqueSource, bps: u16) -> TechniqueEvidence {
    TechniqueEvidence {
        source,
        confidence: ConfidenceBps::new(bps).expect("in range"),
    }
}

fn span(technique: SpanTechnique, start: u32, end: u32) -> TechniqueSpan {
    TechniqueSpan {
        technique,
        tick_range: range(start, end),
        evidence: TechniqueEvidence::explicit(),
    }
}

/// A score holding one group, so a single fact is easy to isolate.
fn scored(group: EventGroup) -> Score {
    Score {
        ticks_per_quarter: 480,
        master_bars: vec![bar()],
        tracks: vec![Track {
            name: None,
            channel: 0,
            voices: vec![Voice {
                id: 0,
                event_groups: vec![group],
            }],
            tuning: Tuning::new(Vec::new()),
        }],
        source_meta: None,
        loss: LossReport::new(),
    }
}

fn one_note(atom: AtomNote) -> Score {
    scored(EventGroup {
        kind: EventGroupKind::Single,
        atoms: vec![AtomEvent::Note(atom)],
        technique_spans: Vec::new(),
    })
}

fn write(score: &Score) -> String {
    write_score(score).expect("this score is inside the writer domain")
}

// ── marks: a set, in `NoteMark::ALL` order ─────────────────────────────────

/// Passes already — the byte this slice must not move.
#[test]
fn an_empty_mark_set_keeps_the_inline_note_form() {
    let text = write(&one_note(note(0, 480, 40, 96)));
    assert!(
        text.contains("note { at 0 duration 480 pitch 40 velocity 96 marks [] }"),
        "SWG-4A-04's inline note is unchanged: {text:?}"
    );
}

#[test]
fn a_single_mark_keeps_the_note_inline() {
    let mut atom = note(0, 480, 47, 96);
    atom.marks = marks_of(&[NoteMark::Accent]);
    let text = write(&one_note(atom));
    assert!(
        text.contains("note { at 0 duration 480 pitch 47 velocity 96 marks [accent] }"),
        "a marked note without a position is still one line — §6.1: {text:?}"
    );
}

#[test]
fn marks_are_written_in_note_mark_all_order() {
    let mut atom = note(0, 480, 60, 80);
    atom.marks = marks_of(&NoteMark::ALL);
    let text = write(&one_note(atom));
    assert!(
        text.contains(
            "marks [accent ghost staccato dead_note harmonic_natural harmonic_pinch tap]"
        ),
        "the declaration order of NoteMark::ALL, which is what NoteMarks::iter yields: {text:?}"
    );
}

#[test]
fn marks_are_not_sorted_alphabetically() {
    // `ghost` precedes `dead_note` in `ALL` and follows it alphabetically, so
    // this pair separates the canonical order from the tidy-looking one. A
    // set has no author order to preserve; `ALL` is the only order there is.
    let mut atom = note(0, 480, 60, 80);
    atom.marks = marks_of(&[NoteMark::DeadNote, NoteMark::Ghost]);
    let text = write(&one_note(atom));
    assert!(text.contains("marks [ghost dead_note]"), "{text:?}");
    assert!(!text.contains("marks [dead_note ghost]"), "{text:?}");
}

#[test]
fn the_insertion_order_of_marks_does_not_reach_the_text() {
    let mut forwards = note(0, 480, 60, 80);
    forwards.marks = marks_of(&[NoteMark::Accent, NoteMark::Tap]);
    let mut backwards = note(0, 480, 60, 80);
    backwards.marks = marks_of(&[NoteMark::Tap, NoteMark::Accent]);
    assert_eq!(
        write(&one_note(forwards)),
        write(&one_note(backwards)),
        "the same set writes the same bytes whichever bit was set first"
    );
}

// ── position: the multiline note ───────────────────────────────────────────

#[test]
fn a_note_with_a_position_uses_the_multiline_form() {
    let mut atom = note(720, 160, 52, 80);
    atom.position = Some(NotePosition {
        position: FretboardPosition { string: 4, fret: 2 },
        evidence: evidence(TechniqueSource::InferredFromMidi, 5_000),
    });
    let text = write(&one_note(atom));
    assert!(
        text.contains(
            "\
                note {
                    at 720
                    duration 160
                    pitch 52
                    velocity 80
                    marks []
                    position {
                        string 4
                        fret 2
                        evidence { source inferred_from_midi confidence 5000 }
                    }
                }
"
        ),
        "§6.1's shape, byte for byte: {text:?}"
    );
}

#[test]
fn a_position_string_beyond_the_tuning_is_written_not_refused() {
    // §3 lists this as inhabited. The writer is not a fretboard validator.
    let mut atom = note(0, 480, 60, 80);
    atom.position = Some(NotePosition {
        position: FretboardPosition {
            string: 9,
            fret: 200,
        },
        evidence: TechniqueEvidence::explicit(),
    });
    let text = write(&one_note(atom));
    assert!(text.contains("string 9"), "{text:?}");
    assert!(text.contains("fret 200"), "{text:?}");
}

#[test]
fn position_evidence_is_written_without_normalization() {
    // §2.6: the pairing is not enforced by the type, so both odd
    // combinations are model-valid and both must be spellable.
    let mut explicit_at_zero = note(0, 480, 60, 80);
    explicit_at_zero.position = Some(NotePosition {
        position: FretboardPosition { string: 1, fret: 0 },
        evidence: evidence(TechniqueSource::Explicit, 0),
    });
    assert!(
        write(&one_note(explicit_at_zero)).contains("evidence { source explicit confidence 0 }"),
        "explicit at zero confidence is written as it stands"
    );

    let mut inferred_at_max = note(0, 480, 60, 80);
    inferred_at_max.position = Some(NotePosition {
        position: FretboardPosition { string: 1, fret: 0 },
        evidence: evidence(TechniqueSource::InferredFromMidi, 10_000),
    });
    assert!(
        write(&one_note(inferred_at_max))
            .contains("evidence { source inferred_from_midi confidence 10000 }"),
        "and so is inferred at full confidence"
    );
}

// ── technique spans ────────────────────────────────────────────────────────

#[test]
fn every_span_technique_has_its_own_spelling() {
    let techniques = [
        (SpanTechnique::Slide, "span slide {"),
        (SpanTechnique::Bend, "span bend {"),
        (SpanTechnique::Legato, "span legato {"),
        (SpanTechnique::PalmMute, "span palm_mute {"),
        (SpanTechnique::HammerOn, "span hammer_on {"),
        (SpanTechnique::PullOff, "span pull_off {"),
        (SpanTechnique::Vibrato, "span vibrato {"),
        (SpanTechnique::LetRing, "span let_ring {"),
    ];
    for (technique, spelling) in techniques {
        let text = write(&scored(EventGroup {
            kind: EventGroupKind::Single,
            atoms: vec![AtomEvent::Note(note(0, 480, 40, 96))],
            technique_spans: vec![span(technique, 0, 480)],
        }));
        assert!(
            text.contains(spelling),
            "{technique:?} is spelled {spelling:?}: {text:?}"
        );
    }
}

#[test]
fn the_eight_span_spellings_are_distinct() {
    // Guards what the loop above cannot see: two techniques mapped to one
    // word satisfy every `contains` and lose a fact the diff reports.
    let mut texts: Vec<String> = [
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
    .map(|technique| {
        write(&scored(EventGroup {
            kind: EventGroupKind::Single,
            atoms: Vec::new(),
            technique_spans: vec![span(technique, 0, 480)],
        }))
    })
    .collect();
    texts.sort();
    let before = texts.len();
    texts.dedup();
    assert_eq!(
        before,
        texts.len(),
        "each technique writes a distinct document"
    );
}

#[test]
fn a_span_matches_its_reference_shape() {
    let text = write(&scored(EventGroup {
        kind: EventGroupKind::Chord,
        atoms: Vec::new(),
        technique_spans: vec![span(SpanTechnique::PalmMute, 0, 480)],
    }));
    assert!(
        text.contains(
            "\
                span palm_mute {
                    ticks 0..480
                    evidence { source explicit confidence 10000 }
                }
"
        ),
        "§6.1's shape, byte for byte: {text:?}"
    );
}

#[test]
fn spans_are_written_after_every_atom() {
    // §6.2 rule 1: `atom *` then `span *`, in the order the census lists.
    let text = write(&scored(EventGroup {
        kind: EventGroupKind::Chord,
        atoms: vec![
            AtomEvent::Note(note(0, 480, 40, 96)),
            AtomEvent::Rest(AtomRest {
                absolute_start: Ticks(480),
                duration: Ticks(240),
            }),
        ],
        technique_spans: vec![span(SpanTechnique::PalmMute, 0, 480)],
    }));
    let last_atom = text.find("rest {").expect("the rest is written");
    let first_span = text.find("span ").expect("the span is written");
    assert!(
        last_atom < first_span,
        "every atom precedes every span within one group: {text:?}"
    );
}

#[test]
fn spans_keep_their_vector_order() {
    // §6.2 rule 2: within one repeated slot, encounter order is semantics.
    let text = write(&scored(EventGroup {
        kind: EventGroupKind::Single,
        atoms: Vec::new(),
        technique_spans: vec![
            span(SpanTechnique::Vibrato, 960, 1440),
            span(SpanTechnique::Bend, 0, 480),
        ],
    }));
    let vibrato = text.find("span vibrato").expect("first span");
    let bend = text.find("span bend").expect("second span");
    assert!(
        vibrato < bend,
        "spans are not sorted by technique or by tick: {text:?}"
    );
}

#[test]
fn a_span_range_outside_its_group_is_written_not_refused() {
    // §3 lists this as inhabited: the writer does not check containment.
    let text = write(&scored(EventGroup {
        kind: EventGroupKind::Single,
        atoms: vec![AtomEvent::Note(note(0, 120, 40, 96))],
        technique_spans: vec![span(SpanTechnique::LetRing, 9_000, 99_000)],
    }));
    assert!(text.contains("ticks 9000..99000"), "{text:?}");
}

#[test]
fn span_evidence_is_written_without_normalization() {
    let mut odd = span(SpanTechnique::Slide, 0, 480);
    odd.evidence = evidence(TechniqueSource::Explicit, 0);
    assert!(
        write(&scored(EventGroup {
            kind: EventGroupKind::Single,
            atoms: Vec::new(),
            technique_spans: vec![odd],
        }))
        .contains("evidence { source explicit confidence 0 }"),
        "the writer does not repair an importer's evidence"
    );
}

// ── the domain grows to cover a span's range ───────────────────────────────

#[test]
fn an_inverted_span_tick_range_is_outside_the_writer_domain() {
    // SWG-4A-04 left this out on purpose: the frontier refused spans, so the
    // writer could not emit one and the clause had nothing to guard. Writing
    // them makes §3's sixth clause reachable here, and it must be checked.
    let bad = TechniqueSpan {
        technique: SpanTechnique::Bend,
        // Bypasses `TickRange::new` deliberately — H5: the fields are public.
        tick_range: TickRange {
            start: Ticks(480),
            end: Ticks(0),
        },
        evidence: TechniqueEvidence::explicit(),
    };
    let err = write_score(&scored(EventGroup {
        kind: EventGroupKind::Single,
        atoms: Vec::new(),
        technique_spans: vec![bad],
    }))
    .expect_err("start > end");
    assert!(
        matches!(
            err,
            ExactWriteError::OutsideWriterDomain {
                reason: ValidationError::InvalidTickRange,
                ..
            }
        ),
        "the refusal carries the model's own error: {err:?}"
    );
}

#[test]
fn a_confidence_needs_no_domain_clause() {
    // `ConfidenceBps` keeps its field private and its constructor is the only
    // way in, so `0..=10_000` is enforced by the type. Adding a seventh
    // clause for it would be the writer inventing validation §3 forbids.
    ConfidenceBps::new(10_001).expect_err("10 001 bps is out of range");
    let text = write(&scored(EventGroup {
        kind: EventGroupKind::Single,
        atoms: Vec::new(),
        technique_spans: vec![span(SpanTechnique::Bend, 0, 480)],
    }));
    assert!(text.contains("confidence 10000"), "{text:?}");
}

// ── every musical leaf is observable in the bytes ──────────────────────────

/// A note carrying every musical leaf at once, plus two spans.
fn rich() -> Score {
    scored(EventGroup {
        kind: EventGroupKind::Chord,
        atoms: vec![AtomEvent::Note(AtomNote {
            marks: marks_of(&[NoteMark::Accent]),
            position: Some(NotePosition {
                position: FretboardPosition { string: 4, fret: 2 },
                evidence: evidence(TechniqueSource::InferredFromMidi, 5_000),
            }),
            ..note(0, 480, 40, 96)
        })],
        technique_spans: vec![
            span(SpanTechnique::PalmMute, 0, 480),
            span(SpanTechnique::Vibrato, 480, 960),
        ],
    })
}

fn a_position(string: u8, fret: u8, source: TechniqueSource, bps: u16) -> NotePosition {
    NotePosition {
        position: FretboardPosition { string, fret },
        evidence: evidence(source, bps),
    }
}

/// The path prefix every fact in `rich()` hangs from.
const IN_GROUP: &str = "score.tracks[0].voices[id=0,ordinal=0].event_groups[0]";

/// The three things one canonical fact must satisfy **together**.
///
/// The first version of this matrix checked only the third: the bytes moved.
/// That is half the acceptance contract. `ExactSemanticDiff` is separately
/// well covered in `core/tests/semantic_diff.rs`, but two independent suites
/// agreeing is not a composition witness — nothing said the fact the writer
/// spells and the fact the comparator sees are the *same* fact. This binds
/// them at one mutation.
///
/// Expected paths are read off the walker rather than off its output:
/// `diff_note` compares `NotePosition` under one `Position` option leaf and
/// records the fretboard mismatch inside that scope with no further segment,
/// so `string` and `fret` both surface at `.position`. `diff_span` compares
/// `TechniqueEvidence` whole. Neither is a defect to route around — the
/// comparator's composite fields are deliberate, and 4A-05 does not touch it.
fn assert_observable(label: &str, expected_path: &str, edit: impl FnOnce(&mut Score)) {
    let before = rich();
    let mut after = before.clone();
    edit(&mut after);

    let paths: Vec<String> = exact_semantic_diff(&before, &after)
        .differences
        .iter()
        .map(|difference| difference.path.to_string())
        .collect();
    assert!(
        paths.iter().any(|path| path == expected_path),
        "{label}: the exact diff must report `{expected_path}`, got {paths:?}"
    );
    assert_ne!(
        write(&before),
        write(&after),
        "{label}: the writer's bytes must move too"
    );
}

#[test]
fn every_note_leaf_is_one_fact_to_the_diff_and_to_the_writer() {
    assert_observable("marks", &format!("{IN_GROUP}.atoms[0].marks"), |score| {
        edit_note(score, |n| n.marks = marks_of(&[NoteMark::Ghost]));
    });
    assert_observable(
        "position presence",
        &format!("{IN_GROUP}.atoms[0].position"),
        |score| {
            edit_note(score, |n| n.position = None);
        },
    );
    assert_observable(
        "position string",
        &format!("{IN_GROUP}.atoms[0].position"),
        |score| {
            edit_note(score, |n| {
                n.position = Some(a_position(5, 2, TechniqueSource::InferredFromMidi, 5_000));
            });
        },
    );
    assert_observable(
        "position fret",
        &format!("{IN_GROUP}.atoms[0].position"),
        |score| {
            edit_note(score, |n| {
                n.position = Some(a_position(4, 3, TechniqueSource::InferredFromMidi, 5_000));
            });
        },
    );
    assert_observable(
        "position evidence source",
        &format!("{IN_GROUP}.atoms[0].position.evidence"),
        |score| {
            edit_note(score, |n| {
                n.position = Some(a_position(4, 2, TechniqueSource::Explicit, 5_000));
            });
        },
    );
    assert_observable(
        "position evidence confidence",
        &format!("{IN_GROUP}.atoms[0].position.evidence"),
        |score| {
            edit_note(score, |n| {
                n.position = Some(a_position(4, 2, TechniqueSource::InferredFromMidi, 5_001));
            });
        },
    );
}

#[test]
fn every_span_fact_is_one_fact_to_the_diff_and_to_the_writer() {
    assert_observable(
        "span presence",
        &format!("{IN_GROUP}.technique_spans"),
        |score| {
            edit_spans(score, |spans| {
                spans.pop();
            });
        },
    );
    assert_observable(
        "span technique",
        &format!("{IN_GROUP}.technique_spans[0].technique"),
        |score| {
            edit_spans(score, |spans| spans[0].technique = SpanTechnique::Legato);
        },
    );
    assert_observable(
        "span tick start",
        &format!("{IN_GROUP}.technique_spans[0].tick_range"),
        |score| {
            edit_spans(score, |spans| spans[0].tick_range = range(24, 480));
        },
    );
    assert_observable(
        "span tick end",
        &format!("{IN_GROUP}.technique_spans[0].tick_range"),
        |score| {
            edit_spans(score, |spans| spans[0].tick_range = range(0, 481));
        },
    );
    assert_observable(
        "span evidence source",
        &format!("{IN_GROUP}.technique_spans[0].evidence"),
        |score| {
            edit_spans(score, |spans| {
                spans[0].evidence = evidence(TechniqueSource::InferredFromMidi, 10_000);
            });
        },
    );
}

/// The gap an independent review found: the matrix moved `source` **and**
/// confidence together, so nothing separated them.
///
/// `rich()`'s spans carry `Explicit` at 10 000. This holds the source still
/// and moves only the confidence, which is the mutation the old
/// `span evidence` row silently did not perform.
#[test]
fn a_span_confidence_alone_is_one_fact_to_the_diff_and_to_the_writer() {
    assert_observable(
        "span evidence confidence",
        &format!("{IN_GROUP}.technique_spans[0].evidence"),
        |score| {
            edit_spans(score, |spans| {
                spans[0].evidence = evidence(TechniqueSource::Explicit, 9_999);
            });
        },
    );
}

/// Order and cardinality do not reduce to one leaf, and should not be forced
/// to: swapping two spans changes both elements, so the honest expectation is
/// that the diff reports under both ordinals and the bytes move.
#[test]
fn span_order_is_visible_to_both_the_diff_and_the_writer() {
    let before = rich();
    let mut after = before.clone();
    edit_spans(&mut after, |spans| spans.swap(0, 1));

    let paths: Vec<String> = exact_semantic_diff(&before, &after)
        .differences
        .iter()
        .map(|difference| difference.path.to_string())
        .collect();
    for ordinal in 0..2 {
        let prefix = format!("{IN_GROUP}.technique_spans[{ordinal}]");
        assert!(
            paths.iter().any(|path| path.starts_with(&prefix)),
            "a swap must be reported under `{prefix}`, got {paths:?}"
        );
    }
    assert_ne!(write(&before), write(&after));
}

/// `rich()` with its one note edited in place.
fn edit_note(score: &mut Score, edit: impl FnOnce(&mut AtomNote)) {
    let group = &mut score.tracks[0].voices[0].event_groups[0];
    let AtomEvent::Note(mut atom) = group.atoms[0] else {
        panic!("the fixture holds a note")
    };
    edit(&mut atom);
    group.atoms[0] = AtomEvent::Note(atom);
}

/// `rich()` with its span vector edited in place.
fn edit_spans(score: &mut Score, edit: impl FnOnce(&mut Vec<TechniqueSpan>)) {
    edit(&mut score.tracks[0].voices[0].event_groups[0].technique_spans);
}
