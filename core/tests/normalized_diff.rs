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
//! - v1 deliberately excludes the projection's loss labels: a musical
//!   policy compares sounding facts, not import provenance;
//! - determinism and directional inversion hold for the normalized report
//!   too.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::missing_assert_message,
    clippy::indexing_slicing,
    clippy::missing_const_for_fn
)]

use griff_core::{
    event::{
        ConfidenceBps, FretboardPosition, NoteMarks, NotePosition, Pitch, SpanTechnique,
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
fn changed_tempo_differs_under_both_contracts() {
    assert_matrix(
        "changed tempo",
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
fn normalized_diff_inverts_directionally() {
    let a = base();
    let mut b = base();
    first_note(&mut b).pitch = Pitch::new(41).expect("valid");
    let forward = normalized_musical_diff(&a, &b);
    let backward = normalized_musical_diff(&b, &a);
    assert_eq!(forward.differences.len(), backward.differences.len());
    for (f, r) in forward.differences.iter().zip(backward.differences.iter()) {
        assert_eq!(f.path, r.path, "typed paths equal in both directions");
        assert_eq!(f.expected, r.actual, "diagnostics swap");
        assert_eq!(f.actual, r.expected);
    }
}
