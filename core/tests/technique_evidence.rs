//! Red → tests for ADR-0018 Slice 2b: span techniques carry evidence.
//!
//! Pins `SpanTechnique` (the renamed `Articulation`, harmonic-free), a
//! `TechniqueEvidence { source, confidence }` with `Explicit`/`InferredFromMidi`
//! sources, and that `TechniqueSpan` carries it. References API that does not
//! exist yet, so the suite fails to compile until the green step.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::missing_assert_message,
    clippy::float_cmp
)]

use griff_core::{
    event::{SpanTechnique, Ticks, TechniqueEvidence, TechniqueSource},
    score::TechniqueSpan,
    slice::TickRange,
};

#[test]
fn evidence_explicit_is_full_confidence() {
    let e = TechniqueEvidence::explicit();
    assert_eq!(e.source, TechniqueSource::Explicit);
    assert_eq!(e.confidence, 1.0);
}

#[test]
fn evidence_inferred_carries_confidence() {
    let e = TechniqueEvidence::inferred(0.6);
    assert_eq!(e.source, TechniqueSource::InferredFromMidi);
    assert_eq!(e.confidence, 0.6);
}

#[test]
fn technique_span_carries_technique_and_evidence() {
    let span = TechniqueSpan {
        technique: SpanTechnique::PalmMute,
        tick_range: TickRange::new(Ticks(0), Ticks(480)).expect("ordered range"),
        evidence: TechniqueEvidence::explicit(),
    };
    assert_eq!(span.technique, SpanTechnique::PalmMute);
    assert_eq!(span.evidence.source, TechniqueSource::Explicit);
}
