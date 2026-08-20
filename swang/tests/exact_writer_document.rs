//! SWG-4A-05 step B: the document-level facts — canonical strings, `source`,
//! and `loss` — committed failing before any implementation.
//!
//! Step A red the musical leaves. This reds the rest, and together they are
//! the whole of what SWG-4A-04's frontier still refuses. One GREEN answers
//! both, because the string encoder is shared: `Track.name`,
//! `SourceMeta.format`, and `ImportWarning::Other` must quote through one
//! path, and splitting the implementation by field would mean holding two
//! theories of quoting at once.
//!
//! §6.5's escaped set is a **frozen range list**, not a Unicode-table
//! predicate. `char::is_control`, `char::is_whitespace`, `escape_default`,
//! and any JSON encoder are all excluded by §1.2: a formatter whose fixed
//! point moved when the build relinked a newer Unicode table would not have
//! one. The boundary table below is what makes that checkable — it walks the
//! edge of every range in the list, from both sides.

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
    TechniqueEvidence, TechniqueSource, Tempo, Ticks, TimeSignature, Tuning, Velocity,
};
use griff_core::score::{
    AtomEvent, AtomNote, AtomRest, EventGroup, EventGroupKind, ImportWarning, LossReport,
    MasterBar, RepeatMarker, Score, SourceMeta, TechniqueSpan, Track, Voice,
};
use griff_core::semantic_diff::{exact_semantic_diff, normalized_musical_diff};
use griff_core::slice::TickRange;
use griff_swang::exact::write_score;

// ── fixtures ───────────────────────────────────────────────────────────────

fn range(start: u32, end: u32) -> TickRange {
    TickRange::new(Ticks(start), Ticks(end)).expect("ordered range")
}

fn bar(index: u64, start: u32, end: u32) -> MasterBar {
    MasterBar {
        index,
        tick_range: range(start, end),
        time_signature: TimeSignature::new(4, 4).expect("4/4"),
        tempo: Tempo::from_bpm_integer(120).expect("120 BPM"),
        repeat: RepeatMarker::default(),
    }
}

/// The smallest score that can carry a `source` or a `loss`.
fn document() -> Score {
    Score {
        ticks_per_quarter: 480,
        master_bars: vec![bar(0, 0, 1920)],
        tracks: Vec::new(),
        source_meta: None,
        loss: LossReport::new(),
    }
}

fn named(name: &str) -> Score {
    let mut score = document();
    score.tracks.push(Track {
        name: Some(name.to_owned()),
        channel: 0,
        voices: Vec::new(),
        tuning: Tuning::new(Vec::new()),
    });
    score
}

fn write(score: &Score) -> String {
    write_score(score).expect("this score is inside the writer domain")
}

/// The canonical spelling of `value` as it appears after `name `.
fn quoted_name(value: &str) -> String {
    let text = write(&named(value));
    let line = text
        .lines()
        .find(|line| line.trim_start().starts_with("name "))
        .expect("the track name is written");
    line.trim_start()
        .strip_prefix("name ")
        .expect("the word")
        .to_owned()
}

// ── §6.5: the frozen escaped set, walked at every edge ─────────────────────

#[test]
fn the_two_always_escaped_characters() {
    assert_eq!(quoted_name("a\"b"), r#""a\"b""#);
    assert_eq!(quoted_name("a\\b"), r#""a\\b""#);
}

#[test]
fn the_three_named_escapes() {
    assert_eq!(quoted_name("a\tb"), r#""a\tb""#);
    assert_eq!(quoted_name("a\nb"), r#""a\nb""#);
    assert_eq!(quoted_name("a\rb"), r#""a\rb""#);
}

#[test]
fn every_enumerated_range_is_escaped_at_both_edges() {
    // §6.5's list, edge by edge:
    //   U+0000..=U+0008   U+000B..=U+000C   U+000E..=U+001F   U+007F..=U+009F
    for (input, expected) in [
        ('\u{0}', r#""\u{0}""#),
        ('\u{8}', r#""\u{8}""#),
        ('\u{b}', r#""\u{b}""#),
        ('\u{c}', r#""\u{c}""#),
        ('\u{e}', r#""\u{e}""#),
        ('\u{1f}', r#""\u{1f}""#),
        ('\u{7f}', r#""\u{7f}""#),
        ('\u{85}', r#""\u{85}""#),
        ('\u{9f}', r#""\u{9f}""#),
    ] {
        assert_eq!(
            quoted_name(&input.to_string()),
            expected,
            "U+{:04X} is inside the enumerated set",
            u32::from(input)
        );
    }
}

#[test]
fn characters_just_outside_the_escaped_set_are_written_through() {
    // The other side of each edge. A predicate that escaped one of these
    // would be `is_control` or `is_whitespace` wearing §6.5's name.
    for outside in [
        '\u{9}',  // TAB — escaped, but by the *named* rule, not the range
        '\u{20}', // space
        '\u{7e}', // ~
        '\u{a0}', // NO-BREAK SPACE: `is_whitespace`, not in §6.5's list
        'é', '中', '🎸',
    ] {
        let quoted = quoted_name(&outside.to_string());
        if outside == '\u{9}' {
            assert_eq!(quoted, r#""\t""#);
            continue;
        }
        assert_eq!(
            quoted,
            format!("\"{outside}\""),
            "U+{:04X} is written through as UTF-8",
            u32::from(outside)
        );
    }
}

#[test]
fn hex_escapes_are_lowercase_and_unpadded() {
    assert_eq!(quoted_name("\u{1f}"), r#""\u{1f}""#);
    assert_ne!(quoted_name("\u{1f}"), r#""\u{1F}""#);
    assert_ne!(quoted_name("\u{1f}"), r#""\u{001f}""#);
    assert_eq!(quoted_name("\u{b}"), r#""\u{b}""#);
    assert_ne!(quoted_name("\u{b}"), r#""\u{0b}""#);
}

#[test]
fn an_empty_and_a_plain_string_need_no_escape_at_all() {
    assert_eq!(quoted_name(""), r#""""#);
    assert_eq!(quoted_name("Guitar"), r#""Guitar""#);
}

// ── one encoder, three call sites ──────────────────────────────────────────

#[test]
fn a_track_name_is_escaped() {
    assert!(write(&named("a\"b\n")).contains(r#"name "a\"b\n""#));
}

#[test]
fn a_source_format_is_escaped() {
    let mut score = document();
    score.source_meta = Some(SourceMeta {
        format: Some("a\"b\n".to_owned()),
    });
    assert!(
        write(&score).contains(r#"source { format "a\"b\n" }"#),
        "the same encoder reaches `source.format`"
    );
}

#[test]
fn an_other_warning_message_is_escaped() {
    let mut score = document();
    score.loss.add(ImportWarning::Other("a\"b\n".to_owned()));
    assert!(
        write(&score).contains(r#"other { message "a\"b\n" }"#),
        "and `loss.other.message` — the third call site"
    );
}

#[test]
fn an_other_warning_stays_one_physical_line() {
    // §6.2: one warning, one line. A U+000A in the value is `\n`, never a
    // real break, so this is a property of the grammar rather than of the
    // data that happens to arrive.
    let mut score = document();
    score
        .loss
        .add(ImportWarning::Other("first\nsecond\nthird".to_owned()));
    let text = write(&score);
    assert_eq!(
        text.lines().filter(|line| line.contains("other {")).count(),
        1
    );
    assert!(
        text.contains(r#"other { message "first\nsecond\nthird" }"#),
        "{text:?}"
    );
}

// ── `source`: three distinguishable states ─────────────────────────────────

#[test]
fn an_absent_source_is_omitted() {
    assert!(!write(&document()).contains("source"));
}

#[test]
fn a_present_source_without_a_format_is_the_empty_inline_block() {
    let mut score = document();
    score.source_meta = Some(SourceMeta { format: None });
    let text = write(&score);
    assert!(text.contains("    source { }\n"), "{text:?}");
    assert!(!text.contains("source {}"), "not the tight form: {text:?}");
    assert!(!text.contains("source {\n"), "not multiline: {text:?}");
}

#[test]
fn the_three_source_states_are_three_documents() {
    let absent = write(&document());

    let mut no_format = document();
    no_format.source_meta = Some(SourceMeta { format: None });

    let mut empty_format = document();
    empty_format.source_meta = Some(SourceMeta {
        format: Some(String::new()),
    });

    let no_format = write(&no_format);
    let empty_format = write(&empty_format);
    assert!(empty_format.contains(r#"source { format "" }"#));
    assert_ne!(absent, no_format);
    assert_ne!(no_format, empty_format);
    assert_ne!(absent, empty_format);
}

// ── `loss`: four variants, vector order, duplicates ────────────────────────

#[test]
fn a_clean_report_writes_no_loss_block() {
    assert!(!write(&document()).contains("loss"));
}

#[test]
fn all_four_warning_variants_match_their_canonical_layout() {
    let mut score = document();
    score.loss.add(ImportWarning::TrackNameInvalidUtf8 {
        track_index: 4_294_967_296,
    });
    score.loss.add(ImportWarning::SmpteTimingUnsupported);
    score.loss.add(ImportWarning::TempoApproximated {
        bar_index: 1,
        nearest_micros: 4_200_000,
    });
    score.loss.add(ImportWarning::Other("example".to_owned()));

    assert!(
        write(&score).contains(
            "\
    loss {
        track_name_invalid_utf8 { track_index 4294967296 }
        smpte_timing_unsupported
        tempo_approximated { bar_index 1 nearest_micros 4200000 }
        other { message \"example\" }
    }
"
        ),
        "§6.2's exhaustive illustration, byte for byte: {:?}",
        write(&score)
    );
}

#[test]
fn both_payload_indices_are_written_past_u32_max() {
    // SWG-CORE-01 widened these to `u64` so that the metadata writer would be
    // built on the final type. Proving it only with small numbers would have
    // made that ordering pointless.
    let mut score = document();
    score.loss.add(ImportWarning::TrackNameInvalidUtf8 {
        track_index: u64::from(u32::MAX) + 1,
    });
    score.loss.add(ImportWarning::TempoApproximated {
        bar_index: u64::MAX,
        nearest_micros: u32::MAX,
    });
    let text = write(&score);
    assert!(text.contains("track_index 4294967296"), "{text:?}");
    assert!(
        text.contains("bar_index 18446744073709551615 nearest_micros 4294967295"),
        "{text:?}"
    );
}

#[test]
fn warnings_keep_their_vector_order() {
    let mut score = document();
    score.loss.add(ImportWarning::Other("second".to_owned()));
    score.loss.add(ImportWarning::SmpteTimingUnsupported);
    score
        .loss
        .add(ImportWarning::TrackNameInvalidUtf8 { track_index: 0 });

    let text = write(&score);
    let positions: Vec<usize> = [
        "other",
        "smpte_timing_unsupported",
        "track_name_invalid_utf8",
    ]
    .iter()
    .map(|needle| text.find(needle).expect("every warning is written"))
    .collect();
    assert!(
        positions.windows(2).all(|w| matches!(w, [a, b] if a < b)),
        "warnings are not grouped by variant: {text:?}"
    );
}

#[test]
fn duplicate_warnings_are_written_twice() {
    let mut score = document();
    for _ in 0..3 {
        score.loss.add(ImportWarning::SmpteTimingUnsupported);
    }
    let text = write(&score);
    assert_eq!(
        text.matches("smpte_timing_unsupported").count(),
        3,
        "the vector holds three and the text carries three: {text:?}"
    );
}

// ── source and loss do not sound, and must survive anyway ──────────────────

#[test]
fn a_source_is_inaudible_to_the_normalized_diff_and_visible_to_the_exact_one() {
    let plain = document();
    let mut with_source = plain.clone();
    with_source.source_meta = Some(SourceMeta {
        format: Some("GP5".to_owned()),
    });

    assert!(
        normalized_musical_diff(&plain, &with_source)
            .differences
            .is_empty(),
        "nothing about the music changed"
    );
    assert!(
        !exact_semantic_diff(&plain, &with_source)
            .differences
            .is_empty(),
        "and the exact contract still sees it"
    );
    assert_ne!(
        write(&plain),
        write(&with_source),
        "so the text must carry it — a receipt keeps the tax line"
    );
}

#[test]
fn a_loss_is_inaudible_to_the_normalized_diff_and_visible_to_the_exact_one() {
    let plain = document();
    let mut with_loss = plain.clone();
    with_loss.loss.add(ImportWarning::SmpteTimingUnsupported);

    assert!(normalized_musical_diff(&plain, &with_loss)
        .differences
        .is_empty());
    assert!(!exact_semantic_diff(&plain, &with_loss)
        .differences
        .is_empty());
    assert_ne!(write(&plain), write(&with_loss));
}

// ── every document-level fact is observable in the bytes ───────────────────

#[test]
fn mutating_any_document_level_fact_changes_the_text() {
    let mut base = document();
    base.source_meta = Some(SourceMeta {
        format: Some("GP5".to_owned()),
    });
    base.loss
        .add(ImportWarning::TrackNameInvalidUtf8 { track_index: 1 });
    base.loss.add(ImportWarning::TempoApproximated {
        bar_index: 2,
        nearest_micros: 500_000,
    });
    base.loss.add(ImportWarning::Other("m".to_owned()));
    let baseline = write(&base);

    let mut format = base.clone();
    format.source_meta = Some(SourceMeta {
        format: Some("MIDI".to_owned()),
    });
    assert_ne!(write(&format), baseline, "the source format is observable");

    let mut variant = base.clone();
    variant.loss.warnings[0] = ImportWarning::SmpteTimingUnsupported;
    assert_ne!(
        write(&variant),
        baseline,
        "the warning variant is observable"
    );

    let mut order = base.clone();
    order.loss.warnings.swap(0, 2);
    assert_ne!(write(&order), baseline, "warning order is observable");

    let mut duplicated = base.clone();
    duplicated
        .loss
        .warnings
        .push(ImportWarning::Other("m".to_owned()));
    assert_ne!(write(&duplicated), baseline, "duplication is observable");

    let mut track_index = base.clone();
    track_index.loss.warnings[0] = ImportWarning::TrackNameInvalidUtf8 { track_index: 2 };
    assert_ne!(write(&track_index), baseline, "track_index is observable");

    let mut bar_index = base.clone();
    bar_index.loss.warnings[1] = ImportWarning::TempoApproximated {
        bar_index: 3,
        nearest_micros: 500_000,
    };
    assert_ne!(write(&bar_index), baseline, "bar_index is observable");

    let mut micros = base.clone();
    micros.loss.warnings[1] = ImportWarning::TempoApproximated {
        bar_index: 2,
        nearest_micros: 500_001,
    };
    assert_ne!(write(&micros), baseline, "nearest_micros is observable");

    let mut message = base;
    message.loss.warnings[2] = ImportWarning::Other("n".to_owned());
    assert_ne!(write(&message), baseline, "the message is observable");
}

// ── the whole reference document, byte for byte ────────────────────────────

/// §6.1's reference score, built in full.
fn reference_score() -> Score {
    let mut second_bar = bar(1, 3840, 7680);
    second_bar.time_signature = TimeSignature::new(7, 8).expect("7/8");
    second_bar.tempo = Tempo::from_micros_per_quarter(4_200_000).expect("100/7 BPM");
    second_bar.repeat = RepeatMarker {
        start: true,
        play_count: 2,
    };

    let note = |at: u32, duration: u32, pitch: u8, velocity: u8| AtomNote {
        absolute_start: Ticks(at),
        duration: Ticks(duration),
        pitch: Pitch::new(pitch).expect("a MIDI pitch"),
        velocity: Velocity::new(velocity).expect("a MIDI velocity"),
        marks: NoteMarks::empty(),
        position: None,
    };

    let mut accented = note(0, 480, 47, 96);
    accented.marks = NoteMarks::empty().with(NoteMark::Accent);

    let mut positioned = note(720, 160, 52, 80);
    positioned.position = Some(NotePosition {
        position: FretboardPosition { string: 4, fret: 2 },
        evidence: TechniqueEvidence {
            source: TechniqueSource::InferredFromMidi,
            confidence: ConfidenceBps::new(5_000).expect("in range"),
        },
    });

    let mut loss = LossReport::new();
    loss.add(ImportWarning::TempoApproximated {
        bar_index: 1,
        nearest_micros: 4_200_000,
    });

    Score {
        ticks_per_quarter: 960,
        master_bars: vec![bar(0, 0, 3840), second_bar],
        tracks: vec![Track {
            name: Some("Guitar".to_owned()),
            channel: 0,
            voices: vec![Voice {
                id: 0,
                event_groups: vec![
                    EventGroup {
                        kind: EventGroupKind::Chord,
                        atoms: vec![
                            AtomEvent::Note(note(0, 480, 40, 96)),
                            AtomEvent::Note(accented),
                        ],
                        technique_spans: vec![TechniqueSpan {
                            technique: SpanTechnique::PalmMute,
                            tick_range: range(0, 480),
                            evidence: TechniqueEvidence::explicit(),
                        }],
                    },
                    EventGroup {
                        kind: EventGroupKind::Single,
                        atoms: vec![AtomEvent::Rest(AtomRest {
                            absolute_start: Ticks(480),
                            duration: Ticks(240),
                        })],
                        technique_spans: Vec::new(),
                    },
                    EventGroup {
                        kind: EventGroupKind::Tuplet { num: 3, den: 2 },
                        atoms: vec![AtomEvent::Note(positioned)],
                        technique_spans: Vec::new(),
                    },
                ],
            }],
            tuning: Tuning::new(
                [64, 59, 55, 50, 45, 40]
                    .into_iter()
                    .map(|p| Pitch::new(p).expect("a MIDI pitch"))
                    .collect(),
            ),
        }],
        source_meta: Some(SourceMeta {
            format: Some("GP5".to_owned()),
        }),
        loss,
    }
}

/// The capstone: §6.1's reference document, produced whole.
///
/// The small tests above and in step A stay, because one large golden is
/// excellent at reporting that something broke and admirably modest about
/// what. This one proves the parts compose — transport, structure, marks,
/// position, span, evidence, source, and loss in one document with nothing
/// between them out of place.
#[test]
fn the_reference_document_is_written_byte_for_byte() {
    assert_eq!(
        write(&reference_score()),
        "\
swang 2

score {
    ppqn 960

    master_bar {
        index 0
        ticks 0..3840
        meter 4/4
        tempo 120/1
    }

    master_bar {
        index 1
        ticks 3840..7680
        meter 7/8
        tempo 100/7
        repeat { start true play_count 2 }
    }

    track {
        name \"Guitar\"
        channel 0
        tuning [64 59 55 50 45 40]

        voice {
            id 0

            group chord {
                note { at 0 duration 480 pitch 40 velocity 96 marks [] }
                note { at 0 duration 480 pitch 47 velocity 96 marks [accent] }
                span palm_mute {
                    ticks 0..480
                    evidence { source explicit confidence 10000 }
                }
            }

            group single {
                rest { at 480 duration 240 }
            }

            group tuplet 3/2 {
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
            }
        }
    }

    source { format \"GP5\" }

    loss {
        tempo_approximated { bar_index 1 nearest_micros 4200000 }
    }
}
"
    );
}
