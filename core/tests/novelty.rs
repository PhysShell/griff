//! Red → green tests for the novelty guard v1
//! (`docs/audit/2026-06-melodic-closure-research.md` §3.4, §7.3).
//!
//! Pins the public `novelty` API: `measure_novelty` compares a candidate
//! track against reference material (corpus chunks) over **transition
//! sequences** — `(interval, normalised inter-onset interval)` pairs of the
//! highest-pitch-per-onset line — so verbatim quotes are caught regardless of
//! transposition or tick resolution:
//!
//! - `NoveltyReport` — raw facts: the longest verbatim quote (notes + which
//!   reference), n-gram counts; the caller's threshold cut reads these.
//! - `novelty_axes` — the ADR-0017 facts: `quote_novelty` (1 − share of the
//!   candidate covered by its longest quote) and `ngram_novelty` (share of
//!   candidate n-grams absent from all references).
//! - `novelty_weights_v1` — the uniform `novelty` v1 policy.
//!
//! References `griff_core::novelty`, which does not exist yet, so the suite
//! fails to compile until the green step.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::missing_assert_message,
    clippy::indexing_slicing,
    clippy::missing_const_for_fn,
    clippy::float_cmp,
    clippy::arithmetic_side_effects,
    clippy::str_to_string
)]

use griff_core::{
    event::{NoteMarks, Pitch, Tempo, Ticks, TimeSignature, Tuning, Velocity},
    novelty::{
        flag_phrase_duplicates, measure_novelty, novelty_axes, novelty_weights_v1, NoveltyError,
        NOVELTY_AXIS_LABELS,
    },
    score::{
        AtomEvent, AtomNote, EventGroup, EventGroupKind, LossReport, MasterBar, RepeatMarker,
        Score, Track, Voice,
    },
    scoring::{rank_indices, Scored},
    slice::TickRange,
};

fn note(start: u32, dur: u32, pitch: u8) -> AtomEvent {
    AtomEvent::Note(AtomNote {
        absolute_start: Ticks(start),
        duration: Ticks(dur),
        pitch: Pitch::new(pitch).expect("valid pitch"),
        velocity: Velocity::new(90).expect("valid velocity"),
        marks: NoteMarks::empty(),
        position: None,
    })
}

fn track_of(atoms: Vec<AtomEvent>) -> Track {
    Track {
        name: None,
        channel: 0,
        voices: vec![Voice {
            id: 0,
            event_groups: atoms
                .into_iter()
                .map(|a| EventGroup {
                    kind: EventGroupKind::Single,
                    atoms: vec![a],
                    technique_spans: Vec::new(),
                })
                .collect(),
        }],
        tuning: Tuning::standard_e(),
    }
}

/// A 4/4 score at `ppqn` whose bars cover all `tracks` content.
fn build_score(ppqn: u16, tracks: Vec<Track>) -> Score {
    let bar = u32::from(ppqn) * 4;
    let end = tracks
        .iter()
        .flat_map(|t| &t.voices)
        .flat_map(|v| &v.event_groups)
        .flat_map(|g| &g.atoms)
        .map(|a| match a {
            AtomEvent::Note(n) => n.absolute_start.0 + n.duration.0,
            AtomEvent::Rest(r) => r.absolute_start.0 + r.duration.0,
        })
        .max()
        .unwrap_or(bar);
    let bar_count = end.div_ceil(bar).max(1) as usize;
    let master_bars = (0..bar_count)
        .map(|i| {
            let start = u32::try_from(i).unwrap() * bar;
            MasterBar {
                index: i,
                tick_range: TickRange::new(Ticks(start), Ticks(start + bar)).expect("ordered"),
                time_signature: TimeSignature {
                    numerator: 4,
                    denominator: 4,
                },
                tempo: Tempo::new(120.0).expect("120 BPM"),
                repeat: RepeatMarker::default(),
            }
        })
        .collect();
    Score {
        ticks_per_quarter: ppqn,
        master_bars,
        tracks,
        source_meta: None,
        loss: LossReport::new(),
    }
}

/// Straight quarter notes over `pitches` at `ppqn`.
fn quarters(ppqn: u16, pitches: &[u8]) -> Vec<AtomEvent> {
    let q = u32::from(ppqn);
    pitches
        .iter()
        .enumerate()
        .map(|(i, &p)| note(u32::try_from(i).unwrap() * q, q, p))
        .collect()
}

const REF_PITCHES: [u8; 8] = [60, 62, 64, 65, 67, 65, 64, 62];

/// The reference melody: eight straight quarters at PPQN 480.
fn reference() -> Score {
    build_score(480, vec![track_of(quarters(480, &REF_PITCHES))])
}

/// Material sharing no transition with the reference: only ±10..12 leaps on
/// an eighth-note grid (the reference moves by ±1..2 on quarters).
fn fresh() -> Score {
    let atoms = vec![
        note(0, 240, 60),
        note(240, 240, 72),
        note(480, 240, 60),
        note(720, 240, 71),
        note(960, 240, 60),
        note(1200, 240, 70),
    ];
    build_score(480, vec![track_of(atoms)])
}

// ── labels and policy ─────────────────────────────────────────────────────────

#[test]
fn axis_labels_are_stable_and_ordered() {
    assert_eq!(NOVELTY_AXIS_LABELS, ["quote_novelty", "ngram_novelty"]);

    let report = measure_novelty(&fresh(), 0, &[reference()]).expect("report");
    let labels: Vec<&str> = novelty_axes(&report).iter().map(|a| a.label).collect();
    assert_eq!(labels, NOVELTY_AXIS_LABELS.to_vec());
}

#[test]
fn weights_v1_is_a_uniform_named_versioned_policy() {
    let policy = novelty_weights_v1();
    assert_eq!(policy.id, "novelty");
    assert_eq!(policy.version, 1);
    for label in NOVELTY_AXIS_LABELS {
        assert_eq!(policy.weight(label), 0.5, "uniform over two axes");
    }
    assert_eq!(policy.weight("no_such_axis"), 0.0);
}

// ── verbatim and disguised quotes are caught ──────────────────────────────────

#[test]
fn a_verbatim_copy_scores_zero_novelty() {
    let candidate = build_score(480, vec![track_of(quarters(480, &REF_PITCHES))]);
    let report = measure_novelty(&candidate, 0, &[reference()]).expect("report");

    assert_eq!(report.candidate_notes, 8);
    assert_eq!(
        report.longest_match_notes, 8,
        "the whole candidate is a quote"
    );
    assert_eq!(report.longest_match_reference, Some(0));

    let axes = novelty_axes(&report);
    assert_eq!(axes.get("quote_novelty").unwrap(), 0.0);
    assert_eq!(axes.get("ngram_novelty").unwrap(), 0.0);
}

#[test]
fn a_transposed_copy_is_still_a_quote() {
    // +5 semitones on every note: intervals and rhythm are identical, so the
    // transition representation must catch it.
    let transposed: Vec<u8> = REF_PITCHES.iter().map(|p| p + 5).collect();
    let candidate = build_score(480, vec![track_of(quarters(480, &transposed))]);
    let report = measure_novelty(&candidate, 0, &[reference()]).expect("report");

    assert_eq!(
        report.longest_match_notes, 8,
        "transposition must not hide a quote"
    );
    assert_eq!(novelty_axes(&report).get("quote_novelty").unwrap(), 0.0);
}

#[test]
fn a_quote_at_a_different_tick_resolution_is_still_a_quote() {
    // The same melody written at PPQN 960: inter-onset intervals normalise
    // onto the same grid.
    let candidate = build_score(480, vec![track_of(quarters(480, &REF_PITCHES))]);
    let reference_960 = build_score(960, vec![track_of(quarters(960, &REF_PITCHES))]);
    let report = measure_novelty(&candidate, 0, &[reference_960]).expect("report");

    assert_eq!(report.longest_match_notes, 8, "PPQN must not hide a quote");
}

#[test]
fn fresh_material_scores_full_novelty() {
    let report = measure_novelty(&fresh(), 0, &[reference()]).expect("report");

    assert_eq!(report.longest_match_notes, 0, "no shared transition at all");
    assert_eq!(report.longest_match_reference, None);

    let axes = novelty_axes(&report);
    assert_eq!(axes.get("quote_novelty").unwrap(), 1.0);
    assert_eq!(axes.get("ngram_novelty").unwrap(), 1.0);
}

// ── partial quotes are measured, not just flagged ─────────────────────────────

#[test]
fn a_partial_quote_yields_exact_shares() {
    // Nine notes: the first five are a verbatim reference quote (transitions
    // t0..t3), the tail leaps by ±12 on eighths (absent from the reference).
    // Candidate transitions: 8 — quote run 4 → quote_novelty = 1 − 4/8 = 0.5.
    // 4-gram windows: 5 total, only t0..t3 matches → ngram_novelty = 0.8.
    let q = 480_u32;
    let atoms = vec![
        note(0, q, 60),
        note(q, q, 62),
        note(2 * q, q, 64),
        note(3 * q, q, 65),
        note(4 * q, q, 67),
        note(5 * q, 240, 79),
        note(5 * q + 240, 240, 67),
        note(5 * q + 480, 240, 79),
        note(5 * q + 720, 240, 67),
    ];
    let candidate = build_score(480, vec![track_of(atoms)]);
    let report = measure_novelty(&candidate, 0, &[reference()]).expect("report");

    assert_eq!(report.candidate_notes, 9);
    assert_eq!(report.longest_match_notes, 5);
    assert_eq!(report.longest_match_reference, Some(0));
    assert_eq!(report.ngram_size, 4);
    assert_eq!(report.candidate_ngrams, 5);
    assert_eq!(report.matched_ngrams, 1);

    let axes = novelty_axes(&report);
    assert_eq!(axes.get("quote_novelty").unwrap(), 0.5);
    assert_eq!(axes.get("ngram_novelty").unwrap(), 0.8);
}

// ── conventions ───────────────────────────────────────────────────────────────

#[test]
fn the_top_note_of_a_chord_carries_the_line() {
    // The reference melody with a low added note under one onset: the
    // highest-pitch-per-onset line is unchanged, so the quote still reads.
    let mut atoms = quarters(480, &REF_PITCHES);
    atoms.push(note(2 * 480, 480, 52));
    let candidate = build_score(480, vec![track_of(atoms)]);
    let report = measure_novelty(&candidate, 0, &[reference()]).expect("report");

    assert_eq!(report.longest_match_notes, 8);
}

#[test]
fn references_use_their_first_note_bearing_track() {
    // Track 0 of the reference is empty; the melody lives on track 1.
    let empty = Track {
        name: None,
        channel: 0,
        voices: vec![Voice {
            id: 0,
            event_groups: Vec::new(),
        }],
        tuning: Tuning::standard_e(),
    };
    let two_track = build_score(480, vec![empty, track_of(quarters(480, &REF_PITCHES))]);
    let candidate = build_score(480, vec![track_of(quarters(480, &REF_PITCHES))]);
    let report = measure_novelty(&candidate, 0, &[two_track]).expect("report");

    assert_eq!(report.longest_match_notes, 8);
    assert_eq!(report.longest_match_reference, Some(0));
}

#[test]
fn the_first_reference_achieving_the_longest_match_wins_ties() {
    let candidate = build_score(480, vec![track_of(quarters(480, &REF_PITCHES))]);
    let report =
        measure_novelty(&candidate, 0, &[fresh(), reference(), reference()]).expect("report");
    assert_eq!(report.longest_match_reference, Some(1), "fixed tie-break");
}

// ── degenerate inputs ─────────────────────────────────────────────────────────

#[test]
fn no_references_means_full_novelty() {
    let candidate = build_score(480, vec![track_of(quarters(480, &REF_PITCHES))]);
    let report = measure_novelty(&candidate, 0, &[]).expect("report");

    assert_eq!(report.longest_match_notes, 0);
    assert_eq!(report.longest_match_reference, None);
    let axes = novelty_axes(&report);
    assert_eq!(axes.get("quote_novelty").unwrap(), 1.0);
    assert_eq!(axes.get("ngram_novelty").unwrap(), 1.0);
}

#[test]
fn a_single_note_candidate_has_nothing_to_quote() {
    let candidate = build_score(480, vec![track_of(quarters(480, &[60]))]);
    let report = measure_novelty(&candidate, 0, &[reference()]).expect("report");

    assert_eq!(report.candidate_notes, 1);
    assert_eq!(report.longest_match_notes, 0);
    let axes = novelty_axes(&report);
    assert_eq!(axes.get("quote_novelty").unwrap(), 1.0);
    assert_eq!(axes.get("ngram_novelty").unwrap(), 1.0);
}

// ── errors ────────────────────────────────────────────────────────────────────

#[test]
fn track_index_out_of_range_is_an_error() {
    assert_eq!(
        measure_novelty(&fresh(), 5, &[reference()]).unwrap_err(),
        NoveltyError::TrackIndexOutOfRange
    );
}

#[test]
fn a_track_with_no_notes_is_an_error() {
    let empty = build_score(
        480,
        vec![Track {
            name: None,
            channel: 0,
            voices: vec![Voice {
                id: 0,
                event_groups: Vec::new(),
            }],
            tuning: Tuning::standard_e(),
        }],
    );
    assert_eq!(
        measure_novelty(&empty, 0, &[reference()]).unwrap_err(),
        NoveltyError::NoNotes
    );
}

// ── determinism and ranking ───────────────────────────────────────────────────

#[test]
fn reports_are_deterministic() {
    let candidate = build_score(480, vec![track_of(quarters(480, &REF_PITCHES))]);
    let refs = [reference(), fresh()];
    let a = measure_novelty(&candidate, 0, &refs).expect("report");
    let b = measure_novelty(&candidate, 0, &refs).expect("report");
    assert_eq!(a, b);
}

#[test]
fn fresh_material_outranks_a_copy() {
    let policy = novelty_weights_v1();
    let refs = [reference()];

    let scored: Vec<Scored<u32>> = [
        build_score(480, vec![track_of(quarters(480, &REF_PITCHES))]),
        fresh(),
    ]
    .iter()
    .enumerate()
    .map(|(i, s)| {
        let report = measure_novelty(s, 0, &refs).expect("report");
        Scored::new(
            u32::try_from(i).unwrap(),
            novelty_axes(&report),
            &policy,
            None,
        )
    })
    .collect();

    let order = rank_indices(&scored);
    assert_eq!(order[0], 1, "the fresh candidate ranks first");
    assert_eq!(scored[1].provenance.policy_id, "novelty");
}

// ── phrase de-duplication (#76) ───────────────────────────────────────────────

#[test]
fn flag_phrase_duplicates_marks_a_later_repeat_as_a_duplicate_of_the_first() {
    // Phrase 0 = the reference melody, 1 = fresh, 2 = a verbatim repeat of 0.
    let phrases = vec![
        build_score(480, vec![track_of(quarters(480, &REF_PITCHES))]),
        fresh(),
        build_score(480, vec![track_of(quarters(480, &REF_PITCHES))]),
    ];
    let flags = flag_phrase_duplicates(&phrases, 0, 0.8);

    assert_eq!(flags.len(), 3);
    assert!(flags[0].is_none(), "the first occurrence is canonical");
    assert!(flags[1].is_none(), "fresh material is not a duplicate");
    let dup = flags[2].expect("the repeat is flagged");
    assert_eq!(dup.of, 0, "flagged as a duplicate of the first phrase");
    assert!(dup.quote_share >= 0.99, "a verbatim repeat is ~100% quote");
}

#[test]
fn flag_phrase_duplicates_leaves_distinct_phrases_unflagged() {
    let phrases = vec![
        build_score(480, vec![track_of(quarters(480, &REF_PITCHES))]),
        fresh(),
    ];
    assert!(flag_phrase_duplicates(&phrases, 0, 0.8)
        .iter()
        .all(Option::is_none));
}

#[test]
fn flag_phrase_duplicates_compares_the_detected_track_on_both_sides() {
    // The detected track is index 1; track 0 is a *different* decoy in each
    // phrase, so comparing track 0 by mistake would not flag — only the repeated
    // track-1 melody should. This guards the single-track reduction: a wrong-track
    // comparison sees mismatched decoys and fails to flag.
    let decoy_a = track_of(quarters(480, &[40, 41, 42, 43]));
    let decoy_b = track_of(quarters(480, &[40, 45, 41, 47]));
    let phrases = vec![
        build_score(480, vec![decoy_a, track_of(quarters(480, &REF_PITCHES))]),
        build_score(480, vec![decoy_b, track_of(quarters(480, &REF_PITCHES))]),
    ];
    let flags = flag_phrase_duplicates(&phrases, 1, 0.8);
    let dup = flags[1].expect("the track-1 repeat is flagged despite a different track 0");
    assert_eq!(dup.of, 0);
    assert!(dup.quote_share >= 0.99, "the track-1 line is a verbatim repeat");
}
