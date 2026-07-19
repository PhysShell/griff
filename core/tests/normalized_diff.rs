//! `NormalizedMusicalDiff` v1 (S16 Phase 4-pre B2): the ADR-0020 projection
//! as a named, versioned comparison policy.
//!
//! Red-phase contract tests:
//!
//! - `normalized_musical_diff` reports under
//!   `SemanticDiffMode::Normalized { policy_id, policy_version }` with the
//!   policy constants `adr-0020-normalized-musical` / `1`;
//! - the comparison pipeline is `Score → NormalizedScore v1 → typed diff`
//!   over the projection's own shape (`bars` / `notes` paths);
//! - the distinction matrix: the same mutation judged by both contracts —
//!   audible changes differ under both, structure/evidence/provenance-only
//!   changes differ under Exact and are equal under Normalized v1;
//! - v1 compares the comparison-relevant musical projection facts,
//!   excluding import-provenance loss labels;
//! - projection-domain boundaries are honestly recorded: with zero tracks
//!   transport has no v1 representation, and with no master bars canonical
//!   notes leave no normalized fact — a boundary of the current ADR-0020
//!   projection, not canonical validity (Phase 4B must validate canonical
//!   input before normalized classification);
//! - order policy: voice order is canonicalized by id, note order by
//!   (onset, string, fret, pitch); track order and bar order remain
//!   positional compared facts;
//! - determinism and directional inversion hold for the normalized report
//!   too.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::missing_assert_message,
    clippy::indexing_slicing,
    clippy::missing_const_for_fn,
    // `.ends_with(".spans")` is a semantic-path suffix, not a file
    // extension; the witness table is one deliberate exhaustive test.
    clippy::case_sensitive_file_extension_comparisons,
    clippy::too_many_lines
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
        exact_semantic_diff, normalized_musical_diff, SemanticDiffMode, SemanticDifferenceKind,
        NORMALIZED_MUSICAL_POLICY_ID, NORMALIZED_MUSICAL_POLICY_VERSION,
    },
    slice::TickRange,
};

// ── fixture ───────────────────────────────────────────────────────────────────

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
        position: Some(NotePosition::explicit(FretboardPosition {
            string: 6,
            fret: pitch.saturating_sub(40),
        })),
    })
}

fn base() -> Score {
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
            name: Some("lead".to_owned()),
            channel: 0,
            voices: vec![Voice {
                id: 0,
                event_groups: vec![EventGroup {
                    kind: EventGroupKind::Single,
                    atoms: vec![note(0, 40), note(480, 43)],
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
        loss: LossReport::new(),
    }
}

fn first_note(score: &mut Score) -> &mut AtomNote {
    match &mut score.tracks[0].voices[0].event_groups[0].atoms[0] {
        AtomEvent::Note(n) => n,
        AtomEvent::Rest(_) => panic!("the fixture's first atom is a note"),
    }
}

/// Runs one mutation through both contracts and asserts the verdict pair.
fn assert_matrix(
    label: &str,
    mutate: impl FnOnce(&mut Score),
    exact_differs: bool,
    normalized_differs: bool,
) {
    let expected = base();
    let mut actual = base();
    mutate(&mut actual);

    let exact = exact_semantic_diff(&expected, &actual);
    assert_eq!(
        !exact.is_empty(),
        exact_differs,
        "{label}: exact verdict, got {:#?}",
        exact.differences
    );

    let normalized = normalized_musical_diff(&expected, &actual);
    assert_eq!(
        !normalized.is_empty(),
        normalized_differs,
        "{label}: normalized v1 verdict, got {:#?}",
        normalized.differences
    );
}

// ── policy identity ───────────────────────────────────────────────────────────

#[test]
fn the_policy_constants_are_pinned() {
    assert_eq!(NORMALIZED_MUSICAL_POLICY_ID, "adr-0020-normalized-musical");
    assert_eq!(NORMALIZED_MUSICAL_POLICY_VERSION, 1);
}

#[test]
fn the_report_carries_the_policy_mode() {
    let report = normalized_musical_diff(&base(), &base());
    assert_eq!(
        report.mode,
        SemanticDiffMode::Normalized {
            policy_id: NORMALIZED_MUSICAL_POLICY_ID,
            policy_version: NORMALIZED_MUSICAL_POLICY_VERSION,
        }
    );
    assert!(report.is_empty(), "identical scores are v1-equal");
}

// ── the distinction matrix ────────────────────────────────────────────────────

#[test]
fn changed_pitch_differs_under_both_contracts() {
    assert_matrix(
        "changed pitch",
        |s| first_note(s).pitch = Pitch::new(41).expect("valid"),
        true,
        true,
    );
}

#[test]
fn changed_onset_differs_under_both_contracts() {
    assert_matrix(
        "changed onset",
        |s| first_note(s).absolute_start = Ticks(240),
        true,
        true,
    );
}

#[test]
fn changed_tempo_on_a_projected_track_differs_under_both_contracts() {
    assert_matrix(
        "changed tempo (projected track)",
        |s| s.master_bars[0].tempo = Tempo::from_bpm_integer(121).expect("121 BPM"),
        true,
        true,
    );
}

#[test]
fn an_added_rest_differs_only_under_exact() {
    assert_matrix(
        "added rest",
        |s| {
            s.tracks[0].voices[0].event_groups[0]
                .atoms
                .push(AtomEvent::Rest(AtomRest {
                    absolute_start: Ticks(960),
                    duration: Ticks(240),
                }));
        },
        true,
        false,
    );
}

#[test]
fn single_replaced_by_chord_differs_only_under_exact() {
    assert_matrix(
        "Single -> Chord",
        |s| s.tracks[0].voices[0].event_groups[0].kind = EventGroupKind::Chord,
        true,
        false,
    );
}

#[test]
fn changed_technique_evidence_differs_only_under_exact() {
    assert_matrix(
        "span evidence explicit -> inferred",
        |s| {
            s.tracks[0].voices[0].event_groups[0].technique_spans[0].evidence =
                TechniqueEvidence::inferred(ConfidenceBps::HALF);
        },
        true,
        false,
    );
}

#[test]
fn changed_source_meta_differs_only_under_exact() {
    assert_matrix(
        "source_meta GP5 -> MIDI",
        |s| {
            s.source_meta = Some(SourceMeta {
                format: Some("MIDI".to_owned()),
            });
        },
        true,
        false,
    );
}

#[test]
fn changed_position_evidence_differs_only_under_exact() {
    assert_matrix(
        "position evidence explicit -> inferred",
        |s| {
            let position = FretboardPosition { string: 6, fret: 0 };
            first_note(s).position = Some(NotePosition::inferred(position, ConfidenceBps::HALF));
        },
        true,
        false,
    );
}

#[test]
fn changed_repeat_marker_differs_only_under_exact() {
    assert_matrix(
        "repeat marker",
        |s| {
            s.master_bars[0].repeat = RepeatMarker {
                start: true,
                play_count: 2,
            };
        },
        true,
        false,
    );
}

#[test]
fn changed_loss_warning_differs_only_under_exact() {
    // v1 is a musical policy: the projection carries loss labels, but the
    // comparator deliberately excludes them — provenance is not music.
    assert_matrix(
        "loss warning",
        |s| {
            s.loss.warnings.push(ImportWarning::SmpteTimingUnsupported);
        },
        true,
        false,
    );
}

#[test]
fn reordered_atoms_differ_only_under_exact() {
    assert_matrix(
        "reordered atoms",
        |s| s.tracks[0].voices[0].event_groups[0].atoms.swap(0, 1),
        true,
        false,
    );
}

// ── projection-domain boundaries (honest v1 losses, not validity) ─────────────

#[test]
fn trackless_transport_differs_only_under_exact() {
    // ADR-0020 materializes transport inside NormTrack.bars; with zero
    // tracks, transport has no representation in v1. An honestly recorded
    // boundary of the current projection — not a claim of canonical
    // validity. Phase 4B must validate canonical input before normalized
    // classification.
    let bare = |bpm: u32| Score {
        ticks_per_quarter: 480,
        master_bars: vec![MasterBar {
            index: 0,
            tick_range: tick_range(0, 1920),
            time_signature: TimeSignature::new(4, 4).expect("4/4"),
            tempo: Tempo::from_bpm_integer(bpm).expect("positive BPM"),
            repeat: RepeatMarker::default(),
        }],
        tracks: Vec::new(),
        source_meta: None,
        loss: LossReport::new(),
    };
    let exact = exact_semantic_diff(&bare(120), &bare(121));
    assert!(!exact.is_empty(), "exact sees the transport change");
    let normalized = normalized_musical_diff(&bare(120), &bare(121));
    assert!(
        normalized.is_empty(),
        "with zero tracks the tempo has no v1 representation: {:#?}",
        normalized.differences
    );
}

#[test]
fn notes_outside_the_master_timeline_differ_only_under_exact() {
    // v1 compares only notes bucketed into declared master bars; canonical
    // notes outside the projected timeline leave no normalized fact. Same
    // honesty clause as above: a projection boundary, not validity.
    let timeline_less = |pitch: u8| {
        let mut score = base();
        score.master_bars = Vec::new();
        match &mut score.tracks[0].voices[0].event_groups[0].atoms[0] {
            AtomEvent::Note(n) => n.pitch = Pitch::new(pitch).expect("valid"),
            AtomEvent::Rest(_) => panic!("fixture note"),
        }
        score
    };
    let exact = exact_semantic_diff(&timeline_less(40), &timeline_less(41));
    assert!(!exact.is_empty(), "exact sees the pitch change");
    let normalized = normalized_musical_diff(&timeline_less(40), &timeline_less(41));
    assert!(
        normalized.is_empty(),
        "without master bars no note is projected: {:#?}",
        normalized.differences
    );
}

// ── order policy: what is canonicalized, what stays positional ────────────────

#[test]
fn voice_order_is_canonicalized_by_id() {
    // The same two voices (content travels with its id), stored in opposite
    // vector orders.
    let voiced = |order: [(u8, u32, u8); 2]| {
        let mut score = base();
        score.tracks[0].voices = order
            .into_iter()
            .map(|(id, onset, pitch)| Voice {
                id,
                event_groups: vec![EventGroup {
                    kind: EventGroupKind::Single,
                    atoms: vec![note(onset, pitch)],
                    technique_spans: Vec::new(),
                }],
            })
            .collect();
        score
    };
    let ascending = voiced([(0, 0, 40), (1, 480, 43)]);
    let descending = voiced([(1, 480, 43), (0, 0, 40)]);
    let exact = exact_semantic_diff(&ascending, &descending);
    assert!(!exact.is_empty(), "voice order is an exact fact");
    let normalized = normalized_musical_diff(&ascending, &descending);
    assert!(
        normalized.is_empty(),
        "v1 canonicalizes voice order by id: {:#?}",
        normalized.differences
    );
}

#[test]
fn note_import_order_is_canonicalized_within_a_voice() {
    let ordered = |reversed: bool| {
        let mut score = base();
        if reversed {
            score.tracks[0].voices[0].event_groups[0].atoms.swap(0, 1);
        }
        score
    };
    let exact = exact_semantic_diff(&ordered(false), &ordered(true));
    assert!(!exact.is_empty(), "atom order is an exact fact");
    let normalized = normalized_musical_diff(&ordered(false), &ordered(true));
    assert!(
        normalized.is_empty(),
        "v1 canonicalizes note order by (onset, string, fret, pitch): {:#?}",
        normalized.differences
    );
}

#[test]
fn track_order_stays_a_positional_fact_under_both_contracts() {
    let two_tracks = |swapped: bool| {
        let mut score = base();
        let second = Track {
            name: Some("rhythm".to_owned()),
            channel: 1,
            voices: vec![Voice {
                id: 0,
                event_groups: vec![EventGroup {
                    kind: EventGroupKind::Single,
                    atoms: vec![note(0, 47)],
                    technique_spans: Vec::new(),
                }],
            }],
            tuning: Tuning::standard_e(),
        };
        score.tracks.push(second);
        if swapped {
            score.tracks.swap(0, 1);
        }
        score
    };
    let exact = exact_semantic_diff(&two_tracks(false), &two_tracks(true));
    assert!(!exact.is_empty());
    let normalized = normalized_musical_diff(&two_tracks(false), &two_tracks(true));
    assert!(
        !normalized.is_empty(),
        "track order is NOT canonicalized by v1 — it stays positional"
    );
}

#[test]
fn bar_order_stays_a_positional_fact_under_both_contracts() {
    let two_bars = |reversed: bool| {
        let mut score = base();
        score.master_bars.push(MasterBar {
            index: 1,
            tick_range: tick_range(1920, 3840),
            time_signature: TimeSignature::new(7, 8).expect("7/8"),
            tempo: Tempo::from_bpm_integer(97).expect("97 BPM"),
            repeat: RepeatMarker::default(),
        });
        if reversed {
            score.master_bars.reverse();
        }
        score
    };
    let exact = exact_semantic_diff(&two_bars(false), &two_bars(true));
    assert!(!exact.is_empty());
    let normalized = normalized_musical_diff(&two_bars(false), &two_bars(true));
    assert!(
        !normalized.is_empty(),
        "bar order is NOT canonicalized by v1 — it stays positional"
    );
}

// ── span semantics: the induced label set at each onset is the fact ───────────

#[test]
fn span_no_longer_covering_an_onset_differs_under_both_at_spans() {
    // Range endpoints and evidence are not preserved; the induced
    // technique-label set at each note onset is. Moving the range off the
    // first note's onset changes that set.
    let expected = base();
    let mut actual = base();
    actual.tracks[0].voices[0].event_groups[0].technique_spans[0].tick_range = tick_range(240, 480);
    let exact = exact_semantic_diff(&expected, &actual);
    assert!(!exact.is_empty(), "the range itself is an exact fact");
    let normalized = normalized_musical_diff(&expected, &actual);
    assert!(!normalized.is_empty(), "the induced label set changed");
    assert!(
        normalized
            .differences
            .iter()
            .any(|d| d.path.to_string().ends_with(".spans")),
        "the normalized difference lands on .spans: {:#?}",
        normalized.differences
    );
}

// ── the projection mutation matrix ────────────────────────────────────────────

/// Asserts that diffing `base()` against its mutation reports a difference
/// with `want_path` and `want_kind`, and that forward and backward runs
/// carry the same typed paths in the same order.
fn assert_projection_witness(
    label: &str,
    mutate: impl FnOnce(&mut Score),
    want_path: &str,
    want_kind: &SemanticDifferenceKind,
) {
    let expected = base();
    let mut actual = base();
    mutate(&mut actual);
    let forward = normalized_musical_diff(&expected, &actual);
    assert!(
        forward
            .differences
            .iter()
            .any(|d| d.path.to_string() == want_path && &d.kind == want_kind),
        "{label}: no difference {want_kind:?} at {want_path}, got {:#?}",
        forward.differences
    );
    let backward = normalized_musical_diff(&actual, &expected);
    let forward_paths: Vec<String> = forward
        .differences
        .iter()
        .map(|d| d.path.to_string())
        .collect();
    let backward_paths: Vec<String> = backward
        .differences
        .iter()
        .map(|d| d.path.to_string())
        .collect();
    assert_eq!(
        forward_paths, backward_paths,
        "{label}: both directions carry the same paths in the same order"
    );
}

#[test]
fn every_projection_field_has_a_witness() {
    let bar0 = "score.tracks[0].bars[index=0,ordinal=0]";
    let voice0 = "score.tracks[0].bars[index=0,ordinal=0].voices[id=0,ordinal=0]";
    let value = SemanticDifferenceKind::ValueMismatch;

    assert_projection_witness("ppqn", |s| s.ticks_per_quarter = 960, "score.ppqn", &value);
    assert_projection_witness(
        "tracks cardinality",
        |s| {
            s.tracks.push(Track {
                name: None,
                channel: 2,
                voices: Vec::new(),
                tuning: Tuning::standard_e(),
            });
        },
        "score.tracks",
        &SemanticDifferenceKind::CardinalityMismatch {
            expected: 1,
            actual: 2,
        },
    );
    assert_projection_witness(
        "name value",
        |s| s.tracks[0].name = Some("solo".to_owned()),
        "score.tracks[0].name",
        &value,
    );
    assert_projection_witness(
        "name dropped",
        |s| s.tracks[0].name = None,
        "score.tracks[0].name",
        &SemanticDifferenceKind::MissingActual,
    );
    assert_projection_witness(
        "channel",
        |s| s.tracks[0].channel = 9,
        "score.tracks[0].channel",
        &value,
    );
    assert_projection_witness(
        "tuning",
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
        &value,
    );
    assert_projection_witness(
        "bars cardinality",
        |s| {
            s.master_bars.push(MasterBar {
                index: 1,
                tick_range: tick_range(1920, 3840),
                time_signature: TimeSignature::new(4, 4).expect("4/4"),
                tempo: Tempo::from_bpm_integer(120).expect("120 BPM"),
                repeat: RepeatMarker::default(),
            });
        },
        "score.tracks[0].bars",
        &SemanticDifferenceKind::CardinalityMismatch {
            expected: 1,
            actual: 2,
        },
    );
    // A disputed NormBar.index drops the annotation: Bar { index: None }.
    assert_projection_witness(
        "bar index (disputed)",
        |s| s.master_bars[0].index = 9,
        "score.tracks[0].bars[ordinal=0].index",
        &value,
    );
    assert_projection_witness(
        "time_sig",
        |s| s.master_bars[0].time_signature = TimeSignature::new(3, 4).expect("3/4"),
        &format!("{bar0}.time_sig"),
        &value,
    );
    assert_projection_witness(
        "tempo",
        |s| s.master_bars[0].tempo = Tempo::from_bpm_integer(121).expect("121 BPM"),
        &format!("{bar0}.tempo"),
        &value,
    );
    assert_projection_witness(
        "end_tick",
        |s| s.master_bars[0].tick_range = tick_range(0, 2400),
        &format!("{bar0}.end_tick"),
        &value,
    );
    assert_projection_witness(
        "start_tick",
        |s| s.master_bars[0].tick_range = tick_range(240, 1920),
        &format!("{bar0}.start_tick"),
        &value,
    );
    assert_projection_witness(
        "voices cardinality",
        |s| {
            s.tracks[0].voices.push(Voice {
                id: 1,
                event_groups: vec![EventGroup {
                    kind: EventGroupKind::Single,
                    atoms: vec![note(0, 52)],
                    technique_spans: Vec::new(),
                }],
            });
        },
        &format!("{bar0}.voices"),
        &SemanticDifferenceKind::CardinalityMismatch {
            expected: 1,
            actual: 2,
        },
    );
    // A disputed NormVoice.id drops the annotation: Voice { id: None }.
    assert_projection_witness(
        "voice id (disputed)",
        |s| s.tracks[0].voices[0].id = 2,
        "score.tracks[0].bars[index=0,ordinal=0].voices[ordinal=0].id",
        &value,
    );
    assert_projection_witness(
        "notes cardinality",
        |s| {
            s.tracks[0].voices[0].event_groups[0].atoms.pop();
        },
        &format!("{voice0}.notes"),
        &SemanticDifferenceKind::CardinalityMismatch {
            expected: 2,
            actual: 1,
        },
    );
    assert_projection_witness(
        "onset_tick",
        |s| match &mut s.tracks[0].voices[0].event_groups[0].atoms[1] {
            AtomEvent::Note(n) => n.absolute_start = Ticks(360),
            AtomEvent::Rest(_) => panic!("fixture note"),
        },
        &format!("{voice0}.notes[1].onset_tick"),
        &value,
    );
    assert_projection_witness(
        "dur_tick",
        |s| first_note(s).duration = Ticks(480),
        &format!("{voice0}.notes[0].dur_tick"),
        &value,
    );
    assert_projection_witness(
        "pitch",
        |s| first_note(s).pitch = Pitch::new(41).expect("valid"),
        &format!("{voice0}.notes[0].pitch"),
        &value,
    );
    assert_projection_witness(
        "velocity",
        |s| first_note(s).velocity = Velocity::new(60).expect("valid"),
        &format!("{voice0}.notes[0].velocity"),
        &value,
    );
    assert_projection_witness(
        "string",
        |s| {
            first_note(s).position = Some(NotePosition::explicit(FretboardPosition {
                string: 5,
                fret: 0,
            }));
        },
        &format!("{voice0}.notes[0].string"),
        &value,
    );
    assert_projection_witness(
        "fret",
        |s| {
            first_note(s).position = Some(NotePosition::explicit(FretboardPosition {
                string: 6,
                fret: 5,
            }));
        },
        &format!("{voice0}.notes[0].fret"),
        &value,
    );
    assert_projection_witness(
        "string/fret dropped",
        |s| first_note(s).position = None,
        &format!("{voice0}.notes[0].string"),
        &SemanticDifferenceKind::MissingActual,
    );
    assert_projection_witness(
        "string/fret gained (backward of dropped)",
        |s| first_note(s).position = None,
        &format!("{voice0}.notes[0].fret"),
        &SemanticDifferenceKind::MissingActual,
    );
    assert_projection_witness(
        "marks",
        |s| first_note(s).marks = NoteMarks::empty().with(NoteMark::Accent),
        &format!("{voice0}.notes[0].marks"),
        &value,
    );
    assert_projection_witness(
        "spans",
        |s| {
            s.tracks[0].voices[0].event_groups[0].technique_spans[0].tick_range =
                tick_range(240, 480);
        },
        &format!("{voice0}.notes[0].spans"),
        &value,
    );
}

#[test]
fn a_gained_position_reports_missing_expected_on_string_and_fret() {
    let mut expected = base();
    match &mut expected.tracks[0].voices[0].event_groups[0].atoms[0] {
        AtomEvent::Note(n) => n.position = None,
        AtomEvent::Rest(_) => panic!("fixture note"),
    }
    let actual = base();
    let report = normalized_musical_diff(&expected, &actual);
    let voice0 = "score.tracks[0].bars[index=0,ordinal=0].voices[id=0,ordinal=0]";
    for field in ["string", "fret"] {
        assert!(
            report.differences.iter().any(|d| {
                d.path.to_string() == format!("{voice0}.notes[0].{field}")
                    && d.kind == SemanticDifferenceKind::MissingExpected
            }),
            "{field}: expected MissingExpected, got {:#?}",
            report.differences
        );
    }
}

// ── normalized paths speak the projection's shape ─────────────────────────────

#[test]
fn a_pitch_difference_reports_under_the_projection_path() {
    let expected = base();
    let mut actual = base();
    first_note(&mut actual).pitch = Pitch::new(41).expect("valid");
    let report = normalized_musical_diff(&expected, &actual);
    let paths: Vec<String> = report
        .differences
        .iter()
        .map(|d| d.path.to_string())
        .collect();
    assert!(
        paths.contains(
            &"score.tracks[0].bars[index=0,ordinal=0].voices[id=0,ordinal=0].notes[0].pitch"
                .to_owned()
        ),
        "the normalized path names bars/notes, got {paths:#?}"
    );
}

#[test]
fn a_removed_note_is_a_cardinality_mismatch_on_notes() {
    let expected = base();
    let mut actual = base();
    actual.tracks[0].voices[0].event_groups[0].atoms.pop();
    let report = normalized_musical_diff(&expected, &actual);
    assert!(
        report.differences.iter().any(|d| {
            d.path.to_string()
                == "score.tracks[0].bars[index=0,ordinal=0].voices[id=0,ordinal=0].notes"
                && d.kind
                    == SemanticDifferenceKind::CardinalityMismatch {
                        expected: 2,
                        actual: 1,
                    }
        }),
        "got {:#?}",
        report.differences
    );
}

// ── determinism and inversion ─────────────────────────────────────────────────

#[test]
fn normalized_diff_is_deterministic() {
    let expected = base();
    let mut actual = base();
    first_note(&mut actual).pitch = Pitch::new(41).expect("valid");
    let first = normalized_musical_diff(&expected, &actual);
    let second = normalized_musical_diff(&expected, &actual);
    assert_eq!(first, second);
    assert_eq!(format!("{first:?}"), format!("{second:?}"));
}

#[test]
fn normalized_diff_inverts_directionally_across_mixed_kinds() {
    // Value + missing + cardinality in one pair: both directions carry the
    // same typed paths in the same order, kinds invert (ValueMismatch
    // self-inverse, MissingExpected <-> MissingActual, cardinality counts
    // swap), diagnostics swap.
    let a = base();
    let mut b = base();
    first_note(&mut b).pitch = Pitch::new(41).expect("valid");
    b.tracks[0].name = None;
    b.tracks[0].voices[0].event_groups[0].atoms.pop();

    let forward = normalized_musical_diff(&a, &b);
    let backward = normalized_musical_diff(&b, &a);
    assert_eq!(forward.differences.len(), backward.differences.len());
    assert!(forward.differences.len() >= 3);
    for (f, r) in forward.differences.iter().zip(backward.differences.iter()) {
        assert_eq!(f.path, r.path, "typed paths equal, in order");
        let inverted = match &f.kind {
            SemanticDifferenceKind::ValueMismatch => SemanticDifferenceKind::ValueMismatch,
            SemanticDifferenceKind::VariantMismatch => SemanticDifferenceKind::VariantMismatch,
            SemanticDifferenceKind::CardinalityMismatch { expected, actual } => {
                SemanticDifferenceKind::CardinalityMismatch {
                    expected: *actual,
                    actual: *expected,
                }
            }
            SemanticDifferenceKind::MissingExpected => SemanticDifferenceKind::MissingActual,
            SemanticDifferenceKind::MissingActual => SemanticDifferenceKind::MissingExpected,
        };
        assert_eq!(inverted, r.kind, "kinds invert at {}", f.path);
        assert_eq!(f.expected, r.actual, "diagnostics swap at {}", f.path);
        assert_eq!(f.actual, r.expected);
    }
}
