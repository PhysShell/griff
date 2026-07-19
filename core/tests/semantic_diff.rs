//! ExactSemanticDiff (S16 Phase 4-pre B1): typed path, typed difference
//! kinds, exact recursive comparison of the canonical `Score` tree.
//!
//! Red-phase contract tests:
//!
//! - the path is typed (`SemanticPath` over `SemanticPathSegment` /
//!   `SemanticField`); the string form is presentation only, produced by a
//!   deterministic `Display`;
//! - the result is a report (`SemanticDiffReport { mode, differences }`),
//!   not a bare `Vec`;
//! - the mutation matrix: one controlled mutation per canonical `Score`
//!   field yields the expected typed path and kind;
//! - exactness: no auto-sort, no fuzzy matching — reorders are differences;
//! - determinism and directional inversion.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::missing_assert_message,
    clippy::indexing_slicing
)]

use griff_core::{
    event::{
        ConfidenceBps, FretboardPosition, NoteMark, NoteMarks, NotePosition, Pitch, SpanTechnique,
        TechniqueEvidence, Tempo, Ticks, TimeSignature, Tuning, Velocity,
    },
    score::{
        AtomEvent, AtomNote, AtomRest, EventGroup, EventGroupKind, ImportWarning, LossReport,
        MasterBar, RepeatMarker, Score, SourceMeta, TechniqueSpan, Track, Voice,
    },
    semantic_diff::{
        exact_semantic_diff, SemanticDiffMode, SemanticDiffReport, SemanticDifference,
        SemanticDifferenceKind, SemanticPathSegment,
    },
    slice::TickRange,
};

// ── fixture ───────────────────────────────────────────────────────────────────

fn tick_range(start: u32, end: u32) -> TickRange {
    TickRange::new(Ticks(start), Ticks(end)).expect("ordered range")
}

fn positioned_note(onset: u32, string: u8, fret: u8, pitch: u8) -> AtomEvent {
    AtomEvent::Note(AtomNote {
        absolute_start: Ticks(onset),
        duration: Ticks(240),
        pitch: Pitch::new(pitch).expect("valid pitch"),
        velocity: Velocity::new(96).expect("valid velocity"),
        marks: NoteMarks::empty().with(NoteMark::Accent),
        position: Some(NotePosition::explicit(FretboardPosition { string, fret })),
    })
}

fn open_note(onset: u32, pitch: u8) -> AtomEvent {
    AtomEvent::Note(AtomNote {
        absolute_start: Ticks(onset),
        duration: Ticks(240),
        pitch: Pitch::new(pitch).expect("valid pitch"),
        velocity: Velocity::new(80).expect("valid velocity"),
        marks: NoteMarks::empty(),
        position: None,
    })
}

fn rest(onset: u32, duration: u32) -> AtomEvent {
    AtomEvent::Rest(AtomRest {
        absolute_start: Ticks(onset),
        duration: Ticks(duration),
    })
}

/// A base score touching every compared fact: two bars (one with a repeat),
/// two tracks, two voices, positioned and open notes, a rest, marks, a
/// technique span, source metadata, and two loss warnings.
fn base() -> Score {
    Score {
        ticks_per_quarter: 480,
        master_bars: vec![
            MasterBar {
                index: 0,
                tick_range: tick_range(0, 1920),
                time_signature: TimeSignature::new(4, 4).expect("4/4"),
                tempo: Tempo::from_bpm_integer(120).expect("120 BPM"),
                repeat: RepeatMarker {
                    start: true,
                    play_count: 0,
                },
            },
            MasterBar {
                index: 1,
                tick_range: tick_range(1920, 3840),
                time_signature: TimeSignature::new(7, 8).expect("7/8"),
                tempo: Tempo::from_bpm_integer(97).expect("97 BPM"),
                repeat: RepeatMarker {
                    start: false,
                    play_count: 2,
                },
            },
        ],
        tracks: vec![
            Track {
                name: Some("lead".to_owned()),
                channel: 0,
                voices: vec![Voice {
                    id: 0,
                    event_groups: vec![
                        EventGroup {
                            kind: EventGroupKind::Single,
                            atoms: vec![positioned_note(0, 6, 0, 40)],
                            technique_spans: vec![TechniqueSpan {
                                technique: SpanTechnique::PalmMute,
                                tick_range: tick_range(0, 480),
                                evidence: TechniqueEvidence::explicit(),
                            }],
                        },
                        EventGroup {
                            kind: EventGroupKind::Single,
                            atoms: vec![rest(480, 240), open_note(720, 45)],
                            technique_spans: Vec::new(),
                        },
                    ],
                }],
                tuning: Tuning::standard_e(),
            },
            Track {
                name: Some("rhythm".to_owned()),
                channel: 1,
                voices: vec![Voice {
                    id: 1,
                    event_groups: vec![EventGroup {
                        kind: EventGroupKind::Chord,
                        atoms: vec![open_note(0, 47), open_note(0, 52)],
                        technique_spans: Vec::new(),
                    }],
                }],
                tuning: Tuning::standard_e(),
            },
        ],
        source_meta: Some(SourceMeta {
            format: Some("GP5".to_owned()),
        }),
        loss: LossReport {
            warnings: vec![
                ImportWarning::Other("w0".to_owned()),
                ImportWarning::TempoApproximated {
                    bar_index: 0,
                    nearest_micros: 495_868,
                },
            ],
        },
    }
}

/// Diffs `base()` against a mutated copy and unwraps the single difference.
fn single_diff(mutate: impl FnOnce(&mut Score)) -> SemanticDifference {
    let expected = base();
    let mut actual = base();
    mutate(&mut actual);
    let report = exact_semantic_diff(&expected, &actual);
    assert!(
        matches!(report.mode, SemanticDiffMode::Exact),
        "the exact comparator reports mode Exact"
    );
    assert_eq!(
        report.differences.len(),
        1,
        "one controlled mutation is one difference, got {:#?}",
        report.differences
    );
    report
        .differences
        .into_iter()
        .next()
        .expect("one difference")
}

fn assert_diff(
    mutate: impl FnOnce(&mut Score),
    want_path: &str,
    want_kind: &SemanticDifferenceKind,
) {
    let diff = single_diff(mutate);
    assert_eq!(diff.path.to_string(), want_path, "typed path renders as");
    assert_eq!(&diff.kind, want_kind, "difference kind at {want_path}");
}

// ── report shape and fast path ────────────────────────────────────────────────

#[test]
fn identical_scores_produce_an_empty_exact_report() {
    let score = base();
    let report = exact_semantic_diff(&score, &score.clone());
    assert!(matches!(report.mode, SemanticDiffMode::Exact));
    assert!(report.is_empty(), "diff(score, score) = empty");
    assert!(report.differences.is_empty());
}

#[test]
fn the_score_tree_is_fully_eq_for_the_fast_path() {
    fn requires_eq<T: Eq>() {}
    requires_eq::<Score>();
    requires_eq::<SourceMeta>();
    requires_eq::<LossReport>();
}

#[test]
fn the_normalized_mode_shape_is_pinned_for_phase_b2() {
    // B2 will construct this; B1 only pins the shape so it cannot drift into
    // a stringly-typed policy.
    let mode = SemanticDiffMode::Normalized {
        policy_id: "adr-0020-normalized-musical",
        policy_version: 1,
    };
    assert!(matches!(
        mode,
        SemanticDiffMode::Normalized {
            policy_id: "adr-0020-normalized-musical",
            policy_version: 1,
        }
    ));
}

#[test]
fn every_path_is_rooted_at_the_score_segment() {
    let diff = single_diff(|s| s.ticks_per_quarter = 960);
    assert_eq!(
        diff.path.segments().first(),
        Some(&SemanticPathSegment::Score),
        "the typed path starts at the Score root"
    );
}

// ── mutation matrix: score root ───────────────────────────────────────────────

#[test]
fn mutated_ticks_per_quarter_is_a_value_mismatch() {
    let diff = single_diff(|s| s.ticks_per_quarter = 960);
    assert_eq!(diff.path.to_string(), "score.ticks_per_quarter");
    assert_eq!(diff.kind, SemanticDifferenceKind::ValueMismatch);
    assert_eq!(diff.expected.as_deref(), Some("480"));
    assert_eq!(diff.actual.as_deref(), Some("960"));
}

#[test]
fn removed_master_bar_is_a_cardinality_mismatch() {
    assert_diff(
        |s| {
            s.master_bars.pop();
        },
        "score.master_bars",
        &SemanticDifferenceKind::CardinalityMismatch {
            expected: 2,
            actual: 1,
        },
    );
}

// ── mutation matrix: master bar ───────────────────────────────────────────────

#[test]
fn mutated_bar_index_is_reported_under_the_expected_side_coordinates() {
    assert_diff(
        |s| s.master_bars[1].index = 9,
        "score.master_bars[index=1,ordinal=1].index",
        &SemanticDifferenceKind::ValueMismatch,
    );
}

#[test]
fn mutated_bar_tick_range_is_a_value_mismatch() {
    assert_diff(
        |s| s.master_bars[1].tick_range = tick_range(1920, 3000),
        "score.master_bars[index=1,ordinal=1].tick_range",
        &SemanticDifferenceKind::ValueMismatch,
    );
}

#[test]
fn mutated_bar_meter_is_a_value_mismatch() {
    assert_diff(
        |s| s.master_bars[0].time_signature = TimeSignature::new(3, 4).expect("3/4"),
        "score.master_bars[index=0,ordinal=0].time_signature",
        &SemanticDifferenceKind::ValueMismatch,
    );
}

#[test]
fn mutated_bar_tempo_is_a_value_mismatch() {
    assert_diff(
        |s| s.master_bars[0].tempo = Tempo::from_bpm_integer(121).expect("121 BPM"),
        "score.master_bars[index=0,ordinal=0].tempo",
        &SemanticDifferenceKind::ValueMismatch,
    );
}

#[test]
fn mutated_bar_repeat_is_a_value_mismatch() {
    assert_diff(
        |s| s.master_bars[1].repeat = RepeatMarker::default(),
        "score.master_bars[index=1,ordinal=1].repeat",
        &SemanticDifferenceKind::ValueMismatch,
    );
}

// ── mutation matrix: track ────────────────────────────────────────────────────

#[test]
fn removed_track_is_a_cardinality_mismatch() {
    assert_diff(
        |s| {
            s.tracks.pop();
        },
        "score.tracks",
        &SemanticDifferenceKind::CardinalityMismatch {
            expected: 2,
            actual: 1,
        },
    );
}

#[test]
fn renamed_track_is_a_value_mismatch() {
    assert_diff(
        |s| s.tracks[1].name = Some("bass".to_owned()),
        "score.tracks[1].name",
        &SemanticDifferenceKind::ValueMismatch,
    );
}

#[test]
fn dropped_track_name_is_missing_actual() {
    let diff = single_diff(|s| s.tracks[0].name = None);
    assert_eq!(diff.path.to_string(), "score.tracks[0].name");
    assert_eq!(diff.kind, SemanticDifferenceKind::MissingActual);
    assert!(diff.expected.is_some(), "the expected side still names it");
    assert!(diff.actual.is_none());
}

#[test]
fn mutated_channel_is_a_value_mismatch() {
    assert_diff(
        |s| s.tracks[0].channel = 9,
        "score.tracks[0].channel",
        &SemanticDifferenceKind::ValueMismatch,
    );
}

#[test]
fn mutated_tuning_is_a_value_mismatch() {
    assert_diff(
        |s| {
            s.tracks[0].tuning = Tuning::new(vec![
                Pitch::new(62).expect("valid"),
                Pitch::new(57).expect("valid"),
                Pitch::new(53).expect("valid"),
                Pitch::new(48).expect("valid"),
                Pitch::new(43).expect("valid"),
                Pitch::new(38).expect("valid"),
            ]);
        },
        "score.tracks[0].tuning",
        &SemanticDifferenceKind::ValueMismatch,
    );
}

// ── mutation matrix: voice ────────────────────────────────────────────────────

#[test]
fn removed_voice_is_a_cardinality_mismatch() {
    assert_diff(
        |s| {
            s.tracks[0].voices.clear();
        },
        "score.tracks[0].voices",
        &SemanticDifferenceKind::CardinalityMismatch {
            expected: 1,
            actual: 0,
        },
    );
}

#[test]
fn mutated_voice_id_is_reported_under_the_expected_side_id() {
    assert_diff(
        |s| s.tracks[1].voices[0].id = 3,
        "score.tracks[1].voices[id=1,ordinal=0].id",
        &SemanticDifferenceKind::ValueMismatch,
    );
}

// ── mutation matrix: event group ──────────────────────────────────────────────

#[test]
fn removed_group_is_a_cardinality_mismatch() {
    assert_diff(
        |s| {
            s.tracks[0].voices[0].event_groups.pop();
        },
        "score.tracks[0].voices[id=0,ordinal=0].event_groups",
        &SemanticDifferenceKind::CardinalityMismatch {
            expected: 2,
            actual: 1,
        },
    );
}

#[test]
fn single_replaced_by_chord_is_a_variant_mismatch_in_exact_mode() {
    assert_diff(
        |s| s.tracks[0].voices[0].event_groups[0].kind = EventGroupKind::Chord,
        "score.tracks[0].voices[id=0,ordinal=0].event_groups[0].kind",
        &SemanticDifferenceKind::VariantMismatch,
    );
}

// ── mutation matrix: atoms ────────────────────────────────────────────────────

#[test]
fn removed_atom_is_a_cardinality_mismatch() {
    assert_diff(
        |s| {
            s.tracks[0].voices[0].event_groups[1].atoms.pop();
        },
        "score.tracks[0].voices[id=0,ordinal=0].event_groups[1].atoms",
        &SemanticDifferenceKind::CardinalityMismatch {
            expected: 2,
            actual: 1,
        },
    );
}

#[test]
fn note_replaced_by_rest_is_a_variant_mismatch() {
    assert_diff(
        |s| s.tracks[0].voices[0].event_groups[1].atoms[1] = rest(720, 240),
        "score.tracks[0].voices[id=0,ordinal=0].event_groups[1].atoms[1].kind",
        &SemanticDifferenceKind::VariantMismatch,
    );
}

fn first_note(score: &mut Score) -> &mut AtomNote {
    match &mut score.tracks[0].voices[0].event_groups[0].atoms[0] {
        AtomEvent::Note(note) => note,
        AtomEvent::Rest(_) => panic!("the fixture's first atom is a note"),
    }
}

#[test]
fn mutated_onset_is_a_value_mismatch() {
    assert_diff(
        |s| first_note(s).absolute_start = Ticks(60),
        "score.tracks[0].voices[id=0,ordinal=0].event_groups[0].atoms[0].absolute_start",
        &SemanticDifferenceKind::ValueMismatch,
    );
}

#[test]
fn mutated_duration_is_a_value_mismatch() {
    assert_diff(
        |s| first_note(s).duration = Ticks(480),
        "score.tracks[0].voices[id=0,ordinal=0].event_groups[0].atoms[0].duration",
        &SemanticDifferenceKind::ValueMismatch,
    );
}

#[test]
fn mutated_pitch_is_a_value_mismatch() {
    assert_diff(
        |s| first_note(s).pitch = Pitch::new(41).expect("valid"),
        "score.tracks[0].voices[id=0,ordinal=0].event_groups[0].atoms[0].pitch",
        &SemanticDifferenceKind::ValueMismatch,
    );
}

#[test]
fn mutated_velocity_is_a_value_mismatch() {
    assert_diff(
        |s| first_note(s).velocity = Velocity::new(60).expect("valid"),
        "score.tracks[0].voices[id=0,ordinal=0].event_groups[0].atoms[0].velocity",
        &SemanticDifferenceKind::ValueMismatch,
    );
}

#[test]
fn mutated_marks_are_a_value_mismatch() {
    assert_diff(
        |s| first_note(s).marks = NoteMarks::empty().with(NoteMark::Ghost),
        "score.tracks[0].voices[id=0,ordinal=0].event_groups[0].atoms[0].marks",
        &SemanticDifferenceKind::ValueMismatch,
    );
}

#[test]
fn dropped_position_is_missing_actual() {
    assert_diff(
        |s| first_note(s).position = None,
        "score.tracks[0].voices[id=0,ordinal=0].event_groups[0].atoms[0].position",
        &SemanticDifferenceKind::MissingActual,
    );
}

#[test]
fn gained_position_is_missing_expected() {
    let expected = base();
    let mut actual = base();
    match &mut actual.tracks[0].voices[0].event_groups[1].atoms[1] {
        AtomEvent::Note(note) => {
            note.position = Some(NotePosition::inferred(
                FretboardPosition { string: 5, fret: 0 },
                ConfidenceBps::HALF,
            ));
        }
        AtomEvent::Rest(_) => panic!("fixture atom 1 of group 1 is a note"),
    }
    let report = exact_semantic_diff(&expected, &actual);
    assert_eq!(report.differences.len(), 1);
    let diff = &report.differences[0];
    assert_eq!(
        diff.path.to_string(),
        "score.tracks[0].voices[id=0,ordinal=0].event_groups[1].atoms[1].position"
    );
    assert_eq!(diff.kind, SemanticDifferenceKind::MissingExpected);
    assert!(diff.expected.is_none());
    assert!(diff.actual.is_some(), "the actual side carries the value");
}

#[test]
fn mutated_position_value_is_a_value_mismatch() {
    assert_diff(
        |s| {
            first_note(s).position = Some(NotePosition::explicit(FretboardPosition {
                string: 5,
                fret: 5,
            }));
        },
        "score.tracks[0].voices[id=0,ordinal=0].event_groups[0].atoms[0].position",
        &SemanticDifferenceKind::ValueMismatch,
    );
}

#[test]
fn mutated_position_evidence_is_reported_under_position_evidence() {
    assert_diff(
        |s| {
            first_note(s).position = Some(NotePosition::inferred(
                FretboardPosition { string: 6, fret: 0 },
                ConfidenceBps::HALF,
            ));
        },
        "score.tracks[0].voices[id=0,ordinal=0].event_groups[0].atoms[0].position.evidence",
        &SemanticDifferenceKind::ValueMismatch,
    );
}

// ── mutation matrix: technique spans ──────────────────────────────────────────

#[test]
fn removed_span_is_a_cardinality_mismatch() {
    assert_diff(
        |s| {
            s.tracks[0].voices[0].event_groups[0]
                .technique_spans
                .clear();
        },
        "score.tracks[0].voices[id=0,ordinal=0].event_groups[0].technique_spans",
        &SemanticDifferenceKind::CardinalityMismatch {
            expected: 1,
            actual: 0,
        },
    );
}

#[test]
fn mutated_span_technique_is_a_variant_mismatch() {
    assert_diff(
        |s| {
            s.tracks[0].voices[0].event_groups[0].technique_spans[0].technique =
                SpanTechnique::LetRing;
        },
        "score.tracks[0].voices[id=0,ordinal=0].event_groups[0].technique_spans[0].technique",
        &SemanticDifferenceKind::VariantMismatch,
    );
}

#[test]
fn mutated_span_range_is_a_value_mismatch() {
    assert_diff(
        |s| {
            s.tracks[0].voices[0].event_groups[0].technique_spans[0].tick_range =
                tick_range(0, 960);
        },
        "score.tracks[0].voices[id=0,ordinal=0].event_groups[0].technique_spans[0].tick_range",
        &SemanticDifferenceKind::ValueMismatch,
    );
}

#[test]
fn mutated_span_evidence_is_a_value_mismatch() {
    assert_diff(
        |s| {
            s.tracks[0].voices[0].event_groups[0].technique_spans[0].evidence =
                TechniqueEvidence::inferred(ConfidenceBps::HALF);
        },
        "score.tracks[0].voices[id=0,ordinal=0].event_groups[0].technique_spans[0].evidence",
        &SemanticDifferenceKind::ValueMismatch,
    );
}

// ── mutation matrix: source meta and loss ─────────────────────────────────────

#[test]
fn dropped_source_meta_is_missing_actual() {
    assert_diff(
        |s| s.source_meta = None,
        "score.source_meta",
        &SemanticDifferenceKind::MissingActual,
    );
}

#[test]
fn mutated_source_meta_is_a_value_mismatch() {
    assert_diff(
        |s| {
            s.source_meta = Some(SourceMeta {
                format: Some("MIDI".to_owned()),
            });
        },
        "score.source_meta",
        &SemanticDifferenceKind::ValueMismatch,
    );
}

#[test]
fn removed_loss_warning_is_a_cardinality_mismatch() {
    assert_diff(
        |s| {
            s.loss.warnings.pop();
        },
        "score.loss.warnings",
        &SemanticDifferenceKind::CardinalityMismatch {
            expected: 2,
            actual: 1,
        },
    );
}

#[test]
fn mutated_loss_warning_is_a_value_mismatch_at_its_ordinal() {
    assert_diff(
        |s| s.loss.warnings[1] = ImportWarning::SmpteTimingUnsupported,
        "score.loss.warnings[1]",
        &SemanticDifferenceKind::ValueMismatch,
    );
}

#[test]
fn reordered_loss_warnings_are_two_value_mismatches() {
    let expected = base();
    let mut actual = base();
    actual.loss.warnings.swap(0, 1);
    let report = exact_semantic_diff(&expected, &actual);
    let paths: Vec<String> = report
        .differences
        .iter()
        .map(|d| d.path.to_string())
        .collect();
    assert_eq!(
        paths,
        vec!["score.loss.warnings[0]", "score.loss.warnings[1]"],
        "warning order is a compared fact"
    );
}

// ── exactness: reorders are differences, never re-sorted away ─────────────────

#[test]
fn reordered_tracks_are_reported_not_normalized() {
    let expected = base();
    let mut actual = base();
    actual.tracks.swap(0, 1);
    let report = exact_semantic_diff(&expected, &actual);
    assert!(
        !report.is_empty(),
        "a track permutation is a real exact difference"
    );
}

#[test]
fn reordered_atoms_are_reported_not_normalized() {
    let expected = base();
    let mut actual = base();
    actual.tracks[1].voices[0].event_groups[0].atoms.swap(0, 1);
    let report = exact_semantic_diff(&expected, &actual);
    assert!(
        !report.is_empty(),
        "an atom permutation is a real exact difference"
    );
}

// ── determinism and depth-first order ─────────────────────────────────────────

#[test]
fn differences_arrive_in_deterministic_depth_first_order() {
    let expected = base();
    let mut actual = base();
    actual.master_bars[0].tempo = Tempo::from_bpm_integer(121).expect("121 BPM");
    actual.ticks_per_quarter = 960;
    match &mut actual.tracks[0].voices[0].event_groups[0].atoms[0] {
        AtomEvent::Note(note) => note.pitch = Pitch::new(41).expect("valid"),
        AtomEvent::Rest(_) => panic!("fixture note"),
    }
    let report = exact_semantic_diff(&expected, &actual);
    let paths: Vec<String> = report
        .differences
        .iter()
        .map(|d| d.path.to_string())
        .collect();
    assert_eq!(
        paths,
        vec![
            "score.ticks_per_quarter",
            "score.master_bars[index=0,ordinal=0].tempo",
            "score.tracks[0].voices[id=0,ordinal=0].event_groups[0].atoms[0].pitch",
        ],
        "declaration order of Score fields, depth-first within each"
    );
}

#[test]
fn repeated_diffs_render_byte_identically() {
    let expected = base();
    let mut actual = base();
    actual.master_bars[0].tempo = Tempo::from_bpm_integer(121).expect("121 BPM");
    let first = exact_semantic_diff(&expected, &actual);
    let second = exact_semantic_diff(&expected, &actual);
    assert_eq!(first, second, "reports compare equal");
    assert_eq!(
        format!("{first:?}"),
        format!("{second:?}"),
        "and render byte-identically"
    );
}

// ── directional inversion ─────────────────────────────────────────────────────

#[test]
fn value_mismatch_inverts_by_swapping_expected_and_actual() {
    let a = base();
    let mut b = base();
    b.master_bars[0].tempo = Tempo::from_bpm_integer(121).expect("121 BPM");

    let forward = exact_semantic_diff(&a, &b);
    let backward = exact_semantic_diff(&b, &a);
    assert_eq!(forward.differences.len(), 1);
    assert_eq!(backward.differences.len(), 1);
    let f = &forward.differences[0];
    let r = &backward.differences[0];
    assert_eq!(f.path.to_string(), r.path.to_string(), "same path");
    assert_eq!(f.kind, r.kind, "ValueMismatch is self-inverse");
    assert_eq!(f.expected, r.actual, "expected/actual swap");
    assert_eq!(f.actual, r.expected);
}

#[test]
fn missing_kinds_invert_into_each_other() {
    let a = base();
    let mut b = base();
    b.source_meta = None;

    let forward = exact_semantic_diff(&a, &b);
    let backward = exact_semantic_diff(&b, &a);
    assert_eq!(
        forward.differences[0].kind,
        SemanticDifferenceKind::MissingActual
    );
    assert_eq!(
        backward.differences[0].kind,
        SemanticDifferenceKind::MissingExpected
    );
    assert_eq!(
        forward.differences[0].path.to_string(),
        backward.differences[0].path.to_string()
    );
}

#[test]
fn cardinality_inverts_by_swapping_counts() {
    let a = base();
    let mut b = base();
    b.tracks.pop();

    let forward = exact_semantic_diff(&a, &b);
    let backward = exact_semantic_diff(&b, &a);
    assert_eq!(
        forward.differences[0].kind,
        SemanticDifferenceKind::CardinalityMismatch {
            expected: 2,
            actual: 1,
        }
    );
    assert_eq!(
        backward.differences[0].kind,
        SemanticDifferenceKind::CardinalityMismatch {
            expected: 1,
            actual: 2,
        }
    );
}

// ── the string form is presentation only ──────────────────────────────────────

#[test]
fn the_display_form_matches_the_documented_examples() {
    let diff = single_diff(|s| s.master_bars[1].tempo = Tempo::from_bpm_integer(60).expect("60"));
    assert_eq!(
        diff.path.to_string(),
        "score.master_bars[index=1,ordinal=1].tempo"
    );

    let diff = single_diff(|s| s.loss.warnings[1] = ImportWarning::SmpteTimingUnsupported);
    assert_eq!(diff.path.to_string(), "score.loss.warnings[1]");
}
