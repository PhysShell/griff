//! Exact scalar semantics (S16 Phase 4-pre A): rational `Tempo` and
//! `ConfidenceBps`.
//!
//! Red-phase contract tests for the exact-scalar API:
//!
//! - `Tempo` is an exact, GCD-normalized rational BPM constructed only from
//!   `from_bpm_integer` / `from_micros_per_quarter`, with `Eq`/`Ord`/`Hash`
//!   and split exact/nearest projections back to MIDI microseconds;
//! - `ConfidenceBps` is a validated `0..=10_000` basis-points confidence;
//!   explicit evidence is `10_000`;
//! - the score tree gains structural equality once no `f64` is left in it;
//! - MIDI export reports a typed approximation loss for a tempo with no
//!   exact microsecond form, instead of rounding silently.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::missing_assert_message,
    clippy::indexing_slicing
)]

use std::collections::HashSet;

use griff_core::{
    event::{
        ConfidenceBps, NoteMarks, Pitch, TechniqueEvidence, TechniqueSource, Tempo, Ticks,
        TimeSignature, Tuning, ValidationError, Velocity,
    },
    midi,
    score::{
        AtomEvent, AtomNote, EventGroup, EventGroupKind, ImportWarning, LossReport, MasterBar,
        RepeatMarker, Score, Track, Voice,
    },
    slice::TickRange,
};

// ── Tempo: construction ───────────────────────────────────────────────────────

#[test]
fn tempo_from_integer_bpm_is_the_reduced_fraction_over_one() {
    let tempo = Tempo::from_bpm_integer(250).expect("250 BPM is valid");
    assert_eq!(tempo.bpm_numerator(), 250);
    assert_eq!(tempo.bpm_denominator(), 1);
}

#[test]
fn tempo_rejects_zero_in_both_constructors() {
    assert_eq!(
        Tempo::from_bpm_integer(0),
        Err(ValidationError::InvalidTempo)
    );
    assert_eq!(
        Tempo::from_micros_per_quarter(0),
        Err(ValidationError::InvalidTempo)
    );
}

#[test]
fn tempo_from_micros_is_gcd_reduced() {
    // 60 000 000 / 500 000 reduces to 120/1.
    let tempo = Tempo::from_micros_per_quarter(500_000).expect("500 000 µs is valid");
    assert_eq!(tempo.bpm_numerator(), 120);
    assert_eq!(tempo.bpm_denominator(), 1);

    // 60 000 000 / 495 868 reduces to 15 000 000 / 123 967.
    let inexact = Tempo::from_micros_per_quarter(495_868).expect("495 868 µs is valid");
    assert_eq!(inexact.bpm_numerator(), 15_000_000);
    assert_eq!(inexact.bpm_denominator(), 123_967);
}

// ── Tempo: equality, ordering, hashing ────────────────────────────────────────

#[test]
fn tempo_equality_is_structural_across_constructors() {
    let from_bpm = Tempo::from_bpm_integer(120).expect("valid");
    let from_micros = Tempo::from_micros_per_quarter(500_000).expect("valid");
    assert_eq!(from_bpm, from_micros);

    let mut set = HashSet::new();
    set.insert(from_bpm);
    set.insert(from_micros);
    assert_eq!(set.len(), 1, "equal tempos must hash equally");
}

#[test]
fn tempo_orders_by_musical_speed() {
    let slow = Tempo::from_bpm_integer(120).expect("valid");
    let fast = Tempo::from_bpm_integer(121).expect("valid");
    assert!(slow < fast);

    // Fewer microseconds per quarter = faster.
    let faster_still = Tempo::from_micros_per_quarter(400_000).expect("valid");
    assert!(fast < faster_still);
}

// ── Tempo: projections ────────────────────────────────────────────────────────

#[test]
fn tempo_micros_round_trip_is_exact_for_every_source_value() {
    for micros in [1_u32, 495_868, 470_003, 500_000, 750_000, 16_777_215] {
        let tempo = Tempo::from_micros_per_quarter(micros).expect("valid micros");
        assert_eq!(
            tempo.to_micros_per_quarter_exact(),
            Some(micros),
            "micros {micros} must project back exactly"
        );
    }
}

#[test]
fn tempo_exact_projection_refuses_a_non_integral_form() {
    // 60 000 000 / 121 is not an integer: there is no exact MIDI form.
    let tempo = Tempo::from_bpm_integer(121).expect("valid");
    assert_eq!(tempo.to_micros_per_quarter_exact(), None);
}

#[test]
fn tempo_nearest_projection_matches_the_characterized_rounding() {
    let tempo = Tempo::from_bpm_integer(121).expect("valid");
    // 495 867.768… rounds to 495 868 — the value the exporter has always
    // written (see tempo_export_characterization).
    assert_eq!(tempo.to_micros_per_quarter_nearest(), 495_868);

    let exact = Tempo::from_bpm_integer(120).expect("valid");
    assert_eq!(exact.to_micros_per_quarter_nearest(), 500_000);
}

#[test]
fn tempo_as_f64_is_the_boundary_projection() {
    let tempo = Tempo::from_bpm_integer(120).expect("valid");
    assert!((tempo.as_f64() - 120.0).abs() < f64::EPSILON);
}

// ── ConfidenceBps ─────────────────────────────────────────────────────────────

#[test]
fn confidence_bps_accepts_the_full_range_and_rejects_beyond() {
    assert_eq!(ConfidenceBps::new(0).map(ConfidenceBps::get), Ok(0));
    assert_eq!(
        ConfidenceBps::new(10_000).map(ConfidenceBps::get),
        Ok(10_000)
    );
    assert_eq!(
        ConfidenceBps::new(10_001),
        Err(ValidationError::ConfidenceOutOfRange { value: 10_001 })
    );
}

#[test]
fn explicit_evidence_carries_maximum_confidence() {
    let evidence = TechniqueEvidence::explicit();
    assert_eq!(evidence.source, TechniqueSource::Explicit);
    assert_eq!(evidence.confidence, ConfidenceBps::MAX);
    assert_eq!(evidence.confidence.get(), 10_000);
}

#[test]
fn inferred_evidence_takes_basis_points() {
    let confidence = ConfidenceBps::new(4_200).expect("in range");
    let evidence = TechniqueEvidence::inferred(confidence);
    assert_eq!(evidence.source, TechniqueSource::InferredFromMidi);
    assert_eq!(evidence.confidence.get(), 4_200);
}

// ── Structural equality over the score tree ───────────────────────────────────

#[test]
fn exact_scalars_unlock_eq_on_the_score_tree() {
    fn requires_eq<T: Eq>() {}
    requires_eq::<Tempo>();
    requires_eq::<ConfidenceBps>();
    requires_eq::<TechniqueEvidence>();
    requires_eq::<AtomNote>();
    requires_eq::<AtomEvent>();
    requires_eq::<EventGroup>();
    requires_eq::<Voice>();
    requires_eq::<Track>();
    requires_eq::<MasterBar>();
}

// ── MIDI export: approximation is a reported fact ─────────────────────────────

/// A minimal one-bar score at the given tempo.
fn one_bar_score(tempo: Tempo) -> Score {
    Score {
        ticks_per_quarter: 480,
        master_bars: vec![MasterBar {
            index: 0,
            tick_range: TickRange::new(Ticks(0), Ticks(1920)).expect("ordered range"),
            time_signature: TimeSignature::new(4, 4).expect("4/4 is valid"),
            tempo,
            repeat: RepeatMarker::default(),
        }],
        tracks: vec![Track {
            name: None,
            channel: 0,
            voices: vec![Voice {
                id: 0,
                event_groups: vec![EventGroup {
                    kind: EventGroupKind::Single,
                    atoms: vec![AtomEvent::Note(AtomNote {
                        absolute_start: Ticks(0),
                        duration: Ticks(480),
                        pitch: Pitch::new(40).expect("valid pitch"),
                        velocity: Velocity::new(96).expect("valid velocity"),
                        marks: NoteMarks::empty(),
                        position: None,
                    })],
                    technique_spans: Vec::new(),
                }],
            }],
            tuning: Tuning::standard_e(),
        }],
        source_meta: None,
        loss: LossReport::new(),
    }
}

#[test]
fn export_of_an_exact_tempo_reports_no_loss() {
    let score = one_bar_score(Tempo::from_bpm_integer(120).expect("valid"));
    let export = midi::export_score(&score).expect("export must succeed");
    assert!(
        export.loss.is_clean(),
        "an exactly representable tempo is not a loss: {:?}",
        export.loss.warnings
    );
    assert!(!export.bytes.is_empty());
}

#[test]
fn export_of_an_inexact_tempo_reports_a_typed_approximation() {
    let score = one_bar_score(Tempo::from_bpm_integer(121).expect("valid"));
    let export = midi::export_score(&score).expect("export must still succeed");
    assert!(
        export
            .loss
            .warnings
            .iter()
            .any(|w| matches!(
                w,
                ImportWarning::TempoApproximated {
                    bar_index: 0,
                    nearest_micros: 495_868,
                }
            )),
        "an inexact MIDI tempo must surface as a typed approximation, got {:?}",
        export.loss.warnings
    );
}
