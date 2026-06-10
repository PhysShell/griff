//! Red → green tests for the S14 Phase-1 structure compiler (ADR-0015).
//!
//! Pins the public API: `StructureControl` (pattern period, repeatability,
//! variation rate) plus `generate_structured` — a tile/vary constraint compiler
//! over the S6 generator, not a new generation core. It generates a
//! `pattern_period`-length base via S6, tiles it across the target span, varies
//! copies deterministically, and returns the produced score together with its
//! measured `StructureMetrics` as provenance. References API that does not
//! exist yet, so the suite fails to compile until the green step.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::missing_assert_message,
    clippy::indexing_slicing,
    clippy::missing_const_for_fn,
    clippy::float_cmp,
    clippy::arithmetic_side_effects
)]

use griff_core::{
    event::{Pitch, Tempo, Ticks, TimeSignature},
    generate::{
        generate, GenerationConstraints, GenerationSeed, GenerationStrategy, PitchMaterial,
        RuleGenerationRequest,
    },
    score::{AtomEvent, Score},
    structure::{generate_structured, StructureControl, StructureGenError, StructuredRequest},
};

const BAR: u32 = 1920; // 4/4 at 480 PPQN

// ── helpers ───────────────────────────────────────────────────────────────────

fn c_major() -> PitchMaterial {
    PitchMaterial {
        root: Pitch(60),
        intervals: vec![0, 2, 4, 5, 7, 9, 11],
    }
}

fn constraints(bar_count: usize) -> GenerationConstraints {
    GenerationConstraints {
        bar_count,
        time_signature: TimeSignature {
            numerator: 4,
            denominator: 4,
        },
        tempo: Tempo(120.0),
        ticks_per_quarter: Ticks(480),
        // Wide range so per-copy transpositions are not clamped away.
        pitch_lo: Pitch(40),
        pitch_hi: Pitch(100),
    }
}

fn request(target_bars: usize, control: StructureControl) -> StructuredRequest {
    StructuredRequest {
        seed: GenerationSeed(7),
        pitch_material: c_major(),
        constraints: constraints(target_bars),
        source_rhythms: vec![vec![Ticks(480); 4]],
        strategy: GenerationStrategy::ConstrainedRandomWalk,
        control,
    }
}

/// All notes of track 0 as `(onset, duration, pitch)`, in onset order.
fn note_tuples(score: &Score) -> Vec<(u32, u32, u8)> {
    let mut out: Vec<(u32, u32, u8)> = score.tracks[0]
        .voices
        .iter()
        .flat_map(|v| &v.event_groups)
        .flat_map(|g| &g.atoms)
        .filter_map(|a| match a {
            AtomEvent::Note(n) => Some((n.absolute_start.0, n.duration.0, n.pitch.0)),
            AtomEvent::Rest(_) => None,
        })
        .collect();
    out.sort_unstable();
    out
}

/// Notes of bar `i` as `(onset - bar_start, duration, pitch)`.
fn bar_tuples(score: &Score, i: usize) -> Vec<(u32, u32, u8)> {
    let start = u32::try_from(i).unwrap() * BAR;
    note_tuples(score)
        .into_iter()
        .filter(|&(onset, _, _)| onset >= start && onset < start + BAR)
        .map(|(onset, d, p)| (onset - start, d, p))
        .collect()
}

// ── tiling ─────────────────────────────────────────────────────────────────────

#[test]
fn high_repeatability_tiles_the_base_bar_verbatim() {
    let control = StructureControl {
        pattern_period_bars: Some(1),
        repeatability: 1.0,
        variation_rate: 0.0,
    };
    let c = generate_structured(&request(4, control)).expect("generate ok");

    assert_eq!(c.score.master_bars.len(), 4, "target span is 4 bars");
    let base = bar_tuples(&c.score, 0);
    assert!(!base.is_empty(), "base bar has material");
    for i in 1..4 {
        assert_eq!(bar_tuples(&c.score, i), base, "bar {i} is a verbatim copy");
    }

    assert_eq!(c.metrics.detected_pattern_period_bars, Some(1));
    assert!(
        c.metrics.repeatability_score > 0.95,
        "verbatim tiles read as high repeatability: {}",
        c.metrics.repeatability_score,
    );
}

#[test]
fn detected_period_recovers_the_requested_period() {
    let control = StructureControl {
        pattern_period_bars: Some(2),
        repeatability: 1.0,
        variation_rate: 0.0,
    };
    let c = generate_structured(&request(8, control)).expect("generate ok");

    assert_eq!(c.score.master_bars.len(), 8);
    assert_ne!(
        bar_tuples(&c.score, 0),
        bar_tuples(&c.score, 1),
        "the 2-bar base must not degenerate into a 1-bar tile",
    );
    assert_eq!(
        c.metrics.detected_pattern_period_bars,
        Some(2),
        "metrics recover the requested period",
    );
    assert_eq!(c.metrics.detected_pattern_period_ticks, Some(2 * BAR));
}

#[test]
fn partial_final_copy_fills_a_non_multiple_span() {
    // Target 5 bars, period 2: two full copies plus the base's first bar.
    let control = StructureControl {
        pattern_period_bars: Some(2),
        repeatability: 1.0,
        variation_rate: 0.0,
    };
    let c = generate_structured(&request(5, control)).expect("generate ok");

    assert_eq!(c.score.master_bars.len(), 5, "span is filled exactly");
    assert_eq!(
        bar_tuples(&c.score, 4),
        bar_tuples(&c.score, 0),
        "the truncated copy starts over from the base's first bar",
    );
}

// ── variation ─────────────────────────────────────────────────────────────────

#[test]
fn variation_transposes_copies_but_preserves_their_rhythm() {
    let control = StructureControl {
        pattern_period_bars: Some(1),
        repeatability: 0.0,
        variation_rate: 1.0,
    };
    let c = generate_structured(&request(4, control)).expect("generate ok");

    let base = bar_tuples(&c.score, 0);
    let base_rhythm: Vec<(u32, u32)> = base.iter().map(|&(o, d, _)| (o, d)).collect();

    let mut any_varied = false;
    for i in 1..4 {
        let copy = bar_tuples(&c.score, i);
        let rhythm: Vec<(u32, u32)> = copy.iter().map(|&(o, d, _)| (o, d)).collect();
        assert_eq!(rhythm, base_rhythm, "variation must not destroy the rhythm");
        if copy != base {
            any_varied = true;
        }
    }
    assert!(any_varied, "zero repeatability must actually vary copies");

    // The contour-aware metric reads transposed tiles as a medium-strength
    // 1-bar period — recognisably the same idea, audibly varied.
    assert_eq!(c.metrics.detected_pattern_period_bars, Some(1));
    assert!(
        (0.6..0.95).contains(&c.metrics.repeatability_score),
        "transposed copies read as medium repeatability: {}",
        c.metrics.repeatability_score,
    );
}

// ── through-composed ──────────────────────────────────────────────────────────

#[test]
fn through_composed_control_delegates_to_s6() {
    let control = StructureControl {
        pattern_period_bars: None,
        repeatability: 0.0,
        variation_rate: 1.0,
    };
    let structured = generate_structured(&request(4, control)).expect("generate ok");

    let plain = generate(&RuleGenerationRequest {
        seed: GenerationSeed(7),
        pitch_material: c_major(),
        constraints: constraints(4),
        source_rhythms: vec![vec![Ticks(480); 4]],
        strategy: GenerationStrategy::ConstrainedRandomWalk,
    })
    .expect("plain S6 ok");

    assert_eq!(
        note_tuples(&structured.score),
        note_tuples(&plain.score),
        "`None` period = through-composed = plain S6 output",
    );
}

// ── contract ──────────────────────────────────────────────────────────────────

#[test]
fn is_deterministic_for_a_fixed_seed_and_control() {
    let control = StructureControl {
        pattern_period_bars: Some(2),
        repeatability: 0.5,
        variation_rate: 0.7,
    };
    let a = generate_structured(&request(8, control)).expect("ok");
    let b = generate_structured(&request(8, control)).expect("ok");
    assert_eq!(note_tuples(&a.score), note_tuples(&b.score));
    assert_eq!(a.metrics, b.metrics);
}

#[test]
fn different_seeds_can_differ() {
    let control = StructureControl {
        pattern_period_bars: Some(1),
        repeatability: 0.0,
        variation_rate: 1.0,
    };
    let mut r1 = request(4, control);
    let mut r2 = request(4, control);
    r1.seed = GenerationSeed(1);
    r2.seed = GenerationSeed(2);
    let a = generate_structured(&r1).expect("ok");
    let b = generate_structured(&r2).expect("ok");
    assert_ne!(
        note_tuples(&a.score),
        note_tuples(&b.score),
        "seeds 1 and 2 should produce different material",
    );
}

#[test]
fn rejects_invalid_controls() {
    let zero_period = StructureControl {
        pattern_period_bars: Some(0),
        repeatability: 1.0,
        variation_rate: 0.0,
    };
    assert!(matches!(
        generate_structured(&request(4, zero_period)),
        Err(StructureGenError::InvalidControl)
    ));

    let period_exceeds_span = StructureControl {
        pattern_period_bars: Some(5),
        repeatability: 1.0,
        variation_rate: 0.0,
    };
    assert!(matches!(
        generate_structured(&request(4, period_exceeds_span)),
        Err(StructureGenError::InvalidControl)
    ));

    let out_of_range = StructureControl {
        pattern_period_bars: Some(1),
        repeatability: 1.5,
        variation_rate: 0.0,
    };
    assert!(matches!(
        generate_structured(&request(4, out_of_range)),
        Err(StructureGenError::InvalidControl)
    ));

    let nan = StructureControl {
        pattern_period_bars: Some(1),
        repeatability: 1.0,
        variation_rate: f64::NAN,
    };
    assert!(matches!(
        generate_structured(&request(4, nan)),
        Err(StructureGenError::InvalidControl)
    ));
}

#[test]
fn propagates_s6_errors() {
    let control = StructureControl {
        pattern_period_bars: Some(1),
        repeatability: 1.0,
        variation_rate: 0.0,
    };
    let mut r = request(4, control);
    r.pitch_material.intervals.clear();
    assert!(matches!(
        generate_structured(&r),
        Err(StructureGenError::Generation(_))
    ));
}

// ── Phase 2: scoring loop (rerank by control ↔ metrics distance) ──────────────

use griff_core::structure::{
    generate_structured_set, rank_structured, structure_axes, structure_weights_v1,
    StructureMetrics, STRUCTURE_AXIS_LABELS,
};

/// Hand-built metrics for pure axis tests.
fn metrics(period: Option<usize>, repeatability: f64) -> StructureMetrics {
    StructureMetrics {
        bar_count: 4,
        detected_pattern_period_bars: period,
        detected_pattern_period_ticks: period.map(|p| u32::try_from(p).unwrap() * BAR),
        repeatability_score: repeatability,
        variation_score: 1.0 - repeatability,
        loopability_score: 0.5,
        structural_complexity: 0.5,
    }
}

#[test]
fn axes_are_perfect_for_an_exact_structure_match() {
    let control = StructureControl {
        pattern_period_bars: Some(1),
        repeatability: 1.0,
        variation_rate: 0.0,
    };
    let c = generate_structured(&request(4, control)).expect("generate ok");

    let axes = structure_axes(&c.control, &c.metrics);
    assert_eq!(axes.len(), STRUCTURE_AXIS_LABELS.len());
    for label in STRUCTURE_AXIS_LABELS {
        assert_eq!(
            axes.get(label),
            Some(1.0),
            "axis {label} must be perfect for a verbatim tile",
        );
    }

    let scored = c.scored(&structure_weights_v1());
    assert!(
        (scored.aggregate() - 1.0).abs() < 1e-9,
        "uniform policy over all-1.0 axes aggregates to 1.0",
    );
}

#[test]
fn period_match_degrades_with_mismatch() {
    let control = |period| StructureControl {
        pattern_period_bars: period,
        repeatability: 1.0,
        variation_rate: 0.0,
    };

    // Requested 2, detected 1 → ratio credit (half).
    let axes = structure_axes(&control(Some(2)), &metrics(Some(1), 1.0));
    assert_eq!(axes.get("period_match"), Some(0.5));

    // Requested a period, detected none → no credit.
    let axes = structure_axes(&control(Some(2)), &metrics(None, 0.1));
    assert_eq!(axes.get("period_match"), Some(0.0));

    // Requested through-composed, material came out periodic → no credit.
    let axes = structure_axes(&control(None), &metrics(Some(1), 1.0));
    assert_eq!(axes.get("period_match"), Some(0.0));

    // Requested through-composed and got it → full credit.
    let axes = structure_axes(&control(None), &metrics(None, 0.1));
    assert_eq!(axes.get("period_match"), Some(1.0));
}

#[test]
fn repeatability_and_variation_axes_measure_distance() {
    let control = StructureControl {
        pattern_period_bars: Some(1),
        repeatability: 0.8,
        variation_rate: 0.3,
    };
    // Measured repeatability 0.5 → distance 0.3 → match 0.7; measured
    // variation 0.5 vs requested 0.3 → match 0.8.
    let axes = structure_axes(&control, &metrics(Some(1), 0.5));
    assert!((axes.get("repeatability_match").unwrap() - 0.7).abs() < 1e-9);
    assert!((axes.get("variation_match").unwrap() - 0.8).abs() < 1e-9);
}

#[test]
fn scored_carries_policy_and_seed_provenance() {
    let control = StructureControl {
        pattern_period_bars: Some(1),
        repeatability: 1.0,
        variation_rate: 0.0,
    };
    let c = generate_structured(&request(4, control)).expect("generate ok");
    let scored = c.scored(&structure_weights_v1());
    assert_eq!(scored.provenance.policy_id, "structure");
    assert_eq!(scored.provenance.policy_version, 1);
    assert_eq!(scored.provenance.seed, Some(7));
}

#[test]
fn rank_puts_the_best_matching_candidate_first() {
    let control = StructureControl {
        pattern_period_bars: Some(1),
        repeatability: 1.0,
        variation_rate: 0.0,
    };
    let good = generate_structured(&request(4, control)).expect("generate ok");

    // Same produced material, but claimed against a control it does not match:
    // a through-composed request that came out as a verbatim 1-bar loop.
    let mut bad = good.clone();
    bad.control = StructureControl {
        pattern_period_bars: None,
        repeatability: 0.0,
        variation_rate: 1.0,
    };

    let policy = structure_weights_v1();
    assert!(
        good.scored(&policy).aggregate() > bad.scored(&policy).aggregate(),
        "the matching candidate must outscore the mismatched one",
    );
    let ranked = rank_structured(&[bad, good], &policy);
    assert_eq!(ranked, vec![1, 0], "best match ranks first");
}

#[test]
fn candidate_set_is_deterministic_with_distinct_seeds() {
    let control = StructureControl {
        pattern_period_bars: Some(1),
        repeatability: 0.5,
        variation_rate: 0.5,
    };
    let a = generate_structured_set(&request(4, control), 3).expect("set ok");
    let b = generate_structured_set(&request(4, control), 3).expect("set ok");

    assert_eq!(a.len(), 3);
    let seeds: Vec<u64> = a.iter().map(|c| c.seed.0).collect();
    assert_eq!(seeds[0], 7, "candidate 0 uses the request seed as-is");
    assert!(seeds[1] != seeds[0] && seeds[2] != seeds[0] && seeds[1] != seeds[2]);

    for (x, y) in a.iter().zip(b.iter()) {
        assert_eq!(x.seed, y.seed);
        assert_eq!(note_tuples(&x.score), note_tuples(&y.score));
        assert_eq!(x.metrics, y.metrics);
    }

    let single = generate_structured(&request(4, control)).expect("ok");
    assert_eq!(
        note_tuples(&a[0].score),
        note_tuples(&single.score),
        "candidate 0 is exactly the single-pass output",
    );
}

#[test]
fn candidate_set_propagates_generation_errors() {
    let control = StructureControl {
        pattern_period_bars: Some(1),
        repeatability: 1.0,
        variation_rate: 0.0,
    };
    let mut r = request(4, control);
    r.pitch_material.intervals.clear();
    assert!(matches!(
        generate_structured_set(&r, 3),
        Err(StructureGenError::Generation(_))
    ));
}
