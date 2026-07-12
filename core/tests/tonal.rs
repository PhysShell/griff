// TDD red phase: the shared pure-core tonal evidence/inference layer
// (TonalContext Phase 1, arbiter 2026-07-12). Generalises the private
// `complement::estimate_harmony` into two separated layers:
//
//   - `PitchEvidence` — raw, observed pitch-class facts for an explicit
//     `EvidenceScope` (whole-score / track / voice): raw onset counts, duration
//     mass in ticks, and the observed `feature::PitchRange`. No thresholds, no
//     key. Additive across scopes (whole score = Σ tracks = Σ voices).
//   - `TonalEstimate` — a ranked 24-key Krumhansl–Schmuckler inference with an
//     explicit `confidence_margin`; each `TonalCandidate` carries tonic, mode,
//     correlation and scale_fit.
//
// KS v1 is duration-only: duration mass weights the histogram; the raw
// onset-count fallback applies *only* when the total duration mass is zero. No
// onset/duration blend and no metric-accent policy in Phase 1.
//
// References `griff_core::tonal`, which does not exist yet, so the suite fails
// to compile until the green step.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::missing_assert_message,
    clippy::indexing_slicing,
    clippy::missing_const_for_fn,
    clippy::float_cmp,
    clippy::arithmetic_side_effects,
    clippy::str_to_string,
    clippy::doc_markdown
)]

use griff_core::{
    event::{NoteMarks, Pitch, Tempo, Ticks, TimeSignature, Tuning, Velocity},
    feature::PitchRange,
    score::{
        AtomEvent, AtomNote, EventGroup, EventGroupKind, LossReport, MasterBar, RepeatMarker,
        Score, Track, Voice,
    },
    slice::TickRange,
    tonal::{estimate_key, EvidenceScope, KeyMode, PitchEvidence},
};

const PPQN: u16 = 480;
const QUARTER: u32 = 480;
const BAR: u32 = 1920; // 4/4 at 480 PPQN

fn quarter_note(start: u32, pitch: u8) -> AtomEvent {
    AtomEvent::Note(AtomNote {
        absolute_start: Ticks(start),
        duration: Ticks(QUARTER),
        pitch: Pitch::new(pitch).expect("valid pitch"),
        velocity: Velocity::new(90).expect("valid velocity"),
        marks: NoteMarks::empty(),
        position: None,
    })
}

fn single_group(atom: AtomEvent) -> EventGroup {
    EventGroup {
        kind: EventGroupKind::Single,
        atoms: vec![atom],
        technique_spans: Vec::new(),
    }
}

fn master_bars(bar_count: usize) -> Vec<MasterBar> {
    (0..bar_count)
        .map(|i| {
            let start = u32::try_from(i).unwrap() * BAR;
            MasterBar {
                index: i,
                tick_range: TickRange::new(Ticks(start), Ticks(start + BAR)).expect("ordered"),
                time_signature: TimeSignature {
                    numerator: 4,
                    denominator: 4,
                },
                tempo: Tempo::new(120.0).expect("120 BPM"),
                repeat: RepeatMarker::default(),
            }
        })
        .collect()
}

fn voice_with(pitches: &[u8]) -> Voice {
    let groups = pitches
        .iter()
        .enumerate()
        .map(|(i, &p)| single_group(quarter_note(u32::try_from(i).unwrap() * QUARTER, p)))
        .collect();
    Voice {
        id: 0,
        event_groups: groups,
    }
}

fn track_with_voices(voices: Vec<Voice>) -> Track {
    Track {
        name: Some("T".to_string()),
        channel: 0,
        voices,
        tuning: Tuning::standard_e(),
    }
}

fn score_with_tracks(tracks: Vec<Track>) -> Score {
    Score {
        ticks_per_quarter: PPQN,
        master_bars: master_bars(1),
        tracks,
        source_meta: None,
        loss: LossReport::new(),
    }
}

// Pitch-class histogram of the clean C major scale with the tonic doubled at the
// octave (C D E F G A B C) — the exact shape complement's key test uses.
const C_MAJOR_COUNTS: [u32; 12] = [2, 0, 1, 0, 1, 1, 0, 1, 0, 1, 0, 1];

// ── evidence: additive across scopes ─────────────────────────────────────────

#[test]
fn whole_score_evidence_is_the_sum_of_its_tracks() {
    let score = score_with_tracks(vec![
        track_with_voices(vec![voice_with(&[60, 64, 67])]), // C E G
        track_with_voices(vec![voice_with(&[62, 65, 69])]), // D F A
    ]);

    let whole = PitchEvidence::measure(&score, EvidenceScope::WholeScore);
    let t0 = PitchEvidence::measure(&score, EvidenceScope::Track(0));
    let t1 = PitchEvidence::measure(&score, EvidenceScope::Track(1));

    assert_eq!(whole.note_count, t0.note_count + t1.note_count);
    assert_eq!(whole.note_count, 6);
    for pc in 0..12 {
        assert_eq!(
            whole.onset_counts[pc],
            t0.onset_counts[pc] + t1.onset_counts[pc],
            "onset counts add across tracks at pc {pc}"
        );
        assert_eq!(
            whole.duration_mass[pc],
            t0.duration_mass[pc] + t1.duration_mass[pc],
            "duration mass adds across tracks at pc {pc}"
        );
    }
    // Whole-score range spans both tracks: C4 (60) .. A4 (69).
    assert_eq!(
        whole.pitch_range,
        Some(PitchRange {
            lowest: Pitch::new(60).unwrap(),
            highest: Pitch::new(69).unwrap(),
        })
    );
}

#[test]
fn track_evidence_is_the_sum_of_its_voices() {
    let score = score_with_tracks(vec![track_with_voices(vec![
        voice_with(&[60, 64]), // voice 0: C E
        voice_with(&[67, 72]), // voice 1: G C
    ])]);

    let track = PitchEvidence::measure(&score, EvidenceScope::Track(0));
    let v0 = PitchEvidence::measure(&score, EvidenceScope::Voice { track: 0, voice: 0 });
    let v1 = PitchEvidence::measure(&score, EvidenceScope::Voice { track: 0, voice: 1 });

    assert_eq!(track.note_count, v0.note_count + v1.note_count);
    assert_eq!(track.note_count, 4);
    for pc in 0..12 {
        assert_eq!(
            track.onset_counts[pc],
            v0.onset_counts[pc] + v1.onset_counts[pc]
        );
        assert_eq!(
            track.duration_mass[pc],
            v0.duration_mass[pc] + v1.duration_mass[pc]
        );
    }
}

#[test]
fn a_silent_scope_yields_empty_evidence_and_no_estimate() {
    let score = score_with_tracks(vec![track_with_voices(vec![voice_with(&[60])])]);

    // Out-of-range track: no notes, no range, no estimate.
    let empty = PitchEvidence::measure(&score, EvidenceScope::Track(9));
    assert_eq!(empty.note_count, 0);
    assert_eq!(empty.onset_counts, [0_u32; 12]);
    assert_eq!(empty.duration_mass, [0_u64; 12]);
    assert_eq!(empty.pitch_range, None);
    assert!(estimate_key(&empty).is_none());
}

// ── inference: 24 ranked candidates, explicit margin ─────────────────────────

#[test]
fn estimate_ranks_all_twenty_four_keys_best_first() {
    let evidence = PitchEvidence {
        scope: EvidenceScope::WholeScore,
        note_count: 8,
        onset_counts: C_MAJOR_COUNTS,
        duration_mass: C_MAJOR_COUNTS.map(|c| u64::from(c) * u64::from(QUARTER)),
        pitch_range: None,
    };

    let est = estimate_key(&evidence).expect("non-empty evidence estimates a key");
    assert_eq!(est.candidates.len(), 24, "all 12 tonics × 2 modes ranked");

    // Descending by correlation.
    for pair in est.candidates.windows(2) {
        assert!(
            pair[0].correlation >= pair[1].correlation,
            "candidates are ranked best-first"
        );
    }

    let winner = &est.candidates[0];
    assert_eq!(winner.tonic, 0, "C");
    assert_eq!(winner.mode, KeyMode::Major);
    assert!(
        (winner.scale_fit - 1.0).abs() < 1e-12,
        "every note diatonic => winner scale_fit 1.0, got {}",
        winner.scale_fit
    );

    // The margin is exactly the winner-vs-runner-up correlation gap, and this
    // clean diatonic case is unambiguous (a real gap).
    assert_eq!(
        est.confidence_margin,
        est.candidates[0].correlation - est.candidates[1].correlation
    );
    assert!(est.confidence_margin > 0.0, "clean C major is unambiguous");
}

#[test]
fn every_candidate_carries_a_scale_fit_in_range() {
    let evidence = PitchEvidence {
        scope: EvidenceScope::WholeScore,
        note_count: 8,
        onset_counts: C_MAJOR_COUNTS,
        duration_mass: C_MAJOR_COUNTS.map(|c| u64::from(c) * u64::from(QUARTER)),
        pitch_range: None,
    };
    let est = estimate_key(&evidence).expect("estimate");
    for c in &est.candidates {
        assert!(
            (0.0..=1.0).contains(&c.scale_fit),
            "scale_fit is a fraction, got {} for tonic {} {:?}",
            c.scale_fit,
            c.tonic,
            c.mode
        );
        assert!(c.correlation.is_finite());
    }
}

// ── flat chromatic histogram: zero confidence, C-major-first, no confidence ──

#[test]
fn an_exactly_flat_histogram_has_zero_correlation_and_zero_margin() {
    // Programmatically flat: every class carries identical onset count *and*
    // identical duration mass. This is the zero-margin characterisation — the
    // deterministic C-major-first order below must NOT be read as confidence.
    let evidence = PitchEvidence {
        scope: EvidenceScope::WholeScore,
        note_count: 12,
        onset_counts: [1_u32; 12],
        duration_mass: [u64::from(QUARTER); 12],
        pitch_range: None,
    };

    let est = estimate_key(&evidence).expect("a full chromatic scope still estimates");
    assert_eq!(est.candidates.len(), 24);
    for c in &est.candidates {
        assert!(c.correlation.is_finite(), "flat histogram stays finite");
        assert_eq!(
            c.correlation, 0.0,
            "a flat histogram correlates with nothing"
        );
    }
    assert_eq!(
        est.confidence_margin, 0.0,
        "no winner over a flat histogram"
    );

    // Deterministic tie order: C major sorts first. This is an ordering
    // convention, not a claim of confidence — the zero margin says so.
    let winner = &est.candidates[0];
    assert_eq!(winner.tonic, 0);
    assert_eq!(winner.mode, KeyMode::Major);
}

// ── KS v1 weighting: duration mass normally, onset-count fallback at zero ─────

#[test]
fn duration_mass_decides_when_present() {
    // Onset counts point at B (all onsets on pc 11); duration mass spells C
    // major. Duration must win in KS v1.
    let evidence = PitchEvidence {
        scope: EvidenceScope::WholeScore,
        note_count: 8,
        onset_counts: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 8],
        duration_mass: C_MAJOR_COUNTS.map(|c| u64::from(c) * u64::from(QUARTER)),
        pitch_range: None,
    };
    let winner = estimate_key(&evidence)
        .expect("estimate")
        .candidates
        .swap_remove(0);
    assert_eq!(
        winner.tonic, 0,
        "duration mass (C major) decides, not onsets"
    );
    assert_eq!(winner.mode, KeyMode::Major);
}

#[test]
fn onset_counts_are_the_fallback_only_when_duration_mass_is_zero() {
    // No duration mass at all: the estimate falls back to raw onset counts and
    // stays defined.
    let evidence = PitchEvidence {
        scope: EvidenceScope::WholeScore,
        note_count: 8,
        onset_counts: C_MAJOR_COUNTS,
        duration_mass: [0_u64; 12],
        pitch_range: None,
    };
    let winner = estimate_key(&evidence)
        .expect("onset fallback keeps the estimate defined")
        .candidates
        .swap_remove(0);
    assert_eq!(winner.tonic, 0, "C");
    assert_eq!(winner.mode, KeyMode::Major);
    assert!((winner.scale_fit - 1.0).abs() < 1e-12);
}

// ── preserved complement winner (characterisation at the tonal layer) ────────

#[test]
fn estimate_reproduces_the_complement_minor_winner_and_tie_order() {
    // E natural minor with the tonic doubled (E3 + E4): pitch-class set equals G
    // major's, so only the tonal weighting picks the minor tonic — the exact
    // case complement pins. The shared estimator must reproduce E minor.
    let e_minor: [u32; 12] = pc_counts(&[52, 55, 57, 59, 60, 62, 64]);
    let evidence = PitchEvidence {
        scope: EvidenceScope::WholeScore,
        note_count: 7,
        onset_counts: e_minor,
        duration_mass: e_minor.map(|c| u64::from(c) * u64::from(QUARTER)),
        pitch_range: None,
    };
    let winner = estimate_key(&evidence)
        .expect("estimate")
        .candidates
        .swap_remove(0);
    assert_eq!(winner.tonic, 4, "E");
    assert_eq!(winner.mode, KeyMode::Minor);
}

fn pc_counts(pitches: &[u8]) -> [u32; 12] {
    let mut counts = [0_u32; 12];
    for &p in pitches {
        counts[usize::from(p) % 12] += 1;
    }
    counts
}
