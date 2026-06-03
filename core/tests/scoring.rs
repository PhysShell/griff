//! Red → tests for the unified scoring vocabulary (ADR-0017).
//!
//! Pins the new `griff_core::scoring` module — `Axes` (facts) kept separate from
//! a named, versioned `WeightPolicy`, an explainable `Rationale`, a derived
//! aggregate, and a stable tie-break — plus the additive complement wiring that
//! exposes `AxisScores` as scoring facts and scores/ranks `ComplementCandidate`s.
//! References API that does not exist yet, so the suite fails to compile until
//! the green step. Pure extension: no existing behaviour or golden is touched.

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
    complement::{
        rank_candidates, relation_weights_v1, AxisScores, ComplementCandidate, RelationMode,
        RELATION_AXIS_LABELS,
    },
    generate::GenerationSeed,
    score::{LossReport, Score},
    scoring::{rank_indices, Axes, Axis, Scored, WeightPolicy},
};

const EPS: f64 = 1e-9;

// ── generic vocabulary ──────────────────────────────────────────────────────

#[test]
fn axes_carry_labelled_facts_and_report_missing() {
    let axes = Axes::new(vec![
        Axis {
            label: "a",
            value: 0.25,
        },
        Axis {
            label: "b",
            value: 0.75,
        },
    ]);
    assert_eq!(axes.len(), 2);
    assert_eq!(axes.get("a"), Some(0.25));
    assert_eq!(axes.get("b"), Some(0.75));
    assert_eq!(axes.get("missing"), None);
}

#[test]
fn weight_policy_is_named_versioned_uniform_and_zero_for_unknown() {
    let policy = WeightPolicy::uniform("p", 3, &["a", "b", "c", "d"]);
    assert_eq!(policy.id, "p");
    assert_eq!(policy.version, 3);
    for label in ["a", "b", "c", "d"] {
        assert!((policy.weight(label) - 0.25).abs() < EPS);
    }
    // Weights are data; a label outside the policy contributes nothing.
    assert_eq!(policy.weight("unknown"), 0.0);
}

#[test]
fn scored_aggregate_is_weighted_sum_of_axes() {
    let policy = WeightPolicy::uniform("p", 1, &["a", "b"]);
    let axes = Axes::new(vec![
        Axis {
            label: "a",
            value: 0.5,
        },
        Axis {
            label: "b",
            value: 1.0,
        },
    ]);
    let scored = Scored::new((), axes, &policy, None);
    // 0.5*0.5 + 1.0*0.5
    assert!((scored.aggregate() - 0.75).abs() < EPS);
}

#[test]
fn rationale_entries_explain_and_sum_to_the_aggregate() {
    let policy = WeightPolicy::uniform("p", 1, &["a", "b"]);
    let axes = Axes::new(vec![
        Axis {
            label: "a",
            value: 0.4,
        },
        Axis {
            label: "b",
            value: 0.8,
        },
    ]);
    let scored = Scored::new((), axes, &policy, None);

    let entries = scored.rationale.entries();
    assert_eq!(entries.len(), 2);
    assert_eq!(entries[0].axis, "a");
    assert_eq!(entries[0].value, 0.4);
    assert!((entries[0].weight - 0.5).abs() < EPS);
    assert!((entries[0].contribution - 0.2).abs() < EPS);

    let sum: f64 = entries.iter().map(|e| e.contribution).sum();
    assert!((sum - scored.aggregate()).abs() < EPS);
}

#[test]
fn provenance_records_policy_version_and_seed() {
    let policy = WeightPolicy::uniform("relation", 1, &["a"]);
    let axes = Axes::new(vec![Axis {
        label: "a",
        value: 1.0,
    }]);
    let scored = Scored::new((), axes, &policy, Some(42));
    assert_eq!(scored.provenance.policy_id, "relation");
    assert_eq!(scored.provenance.policy_version, 1);
    assert_eq!(scored.provenance.seed, Some(42));
}

#[test]
fn rank_indices_orders_by_aggregate_desc_then_stable_index() {
    let policy = WeightPolicy::uniform("p", 1, &["x"]);
    let mk = |v: f64| {
        Scored::new(
            0_usize,
            Axes::new(vec![Axis {
                label: "x",
                value: v,
            }]),
            &policy,
            None,
        )
    };
    // aggregates: [0.2, 0.9, 0.5, 0.9] — a tie between index 1 and 3.
    let scored = vec![mk(0.2), mk(0.9), mk(0.5), mk(0.9)];
    // Descending aggregate; the tie breaks by ascending original index.
    assert_eq!(rank_indices(&scored), vec![1, 3, 2, 0]);
}

// ── complement wiring (additive) ──────────────────────────────────────────────

#[test]
fn axis_scores_expose_relation_axes_as_labelled_data() {
    let ax = AxisScores {
        rhythm_similarity: 1.0,
        register_overlap: 0.25,
        density_ratio: 0.6,
        technique_overlap: 0.0,
    };
    let axes = ax.axes();
    let labels: Vec<&str> = axes.iter().map(|a| a.label).collect();
    assert_eq!(labels, RELATION_AXIS_LABELS.to_vec());
    assert_eq!(axes.get("rhythm_similarity"), Some(1.0));
    assert_eq!(axes.get("density_ratio"), Some(0.6));
    assert_eq!(axes.get("technique_overlap"), Some(0.0));
}

#[test]
fn relation_weights_v1_is_the_named_versioned_baseline() {
    let policy = relation_weights_v1();
    assert_eq!(policy.id, "relation");
    assert_eq!(policy.version, 1);
    for label in RELATION_AXIS_LABELS {
        assert!((policy.weight(label) - 0.25).abs() < EPS);
    }
}

fn empty_score() -> Score {
    Score {
        ticks_per_quarter: 480,
        master_bars: Vec::new(),
        tracks: Vec::new(),
        source_meta: None,
        loss: LossReport::default(),
    }
}

fn candidate_with(axis_scores: AxisScores, part_b_index: usize, seed: u64) -> ComplementCandidate {
    ComplementCandidate {
        score: empty_score(),
        part_b_index,
        mode: RelationMode::RhythmLock,
        seed: GenerationSeed(seed),
        axis_scores,
    }
}

#[test]
fn complement_candidate_scores_reproducibly_with_provenance() {
    let cand = candidate_with(
        AxisScores {
            rhythm_similarity: 1.0,
            register_overlap: 0.5,
            density_ratio: 0.6,
            technique_overlap: 1.0,
        },
        1,
        7,
    );
    let policy = relation_weights_v1();
    let scored = cand.scored(&policy);

    // The produced value is the part-B track locator.
    assert_eq!(scored.value, 1);
    // Provenance pins the seed and the weight-policy version (ADR-0017 §7).
    assert_eq!(scored.provenance.seed, Some(7));
    assert_eq!(scored.provenance.policy_id, "relation");
    assert_eq!(scored.provenance.policy_version, 1);

    let expected = (1.0 + 0.5 + 0.6 + 1.0) / 4.0;
    assert!((scored.aggregate() - expected).abs() < EPS);
    // Same candidate + same policy → same aggregate.
    assert_eq!(cand.scored(&policy).aggregate(), scored.aggregate());
}

#[test]
fn rank_candidates_orders_most_related_first() {
    let weak = candidate_with(
        AxisScores {
            rhythm_similarity: 1.0,
            register_overlap: 0.0,
            density_ratio: 0.0,
            technique_overlap: 0.0,
        },
        1,
        1,
    );
    let strong = candidate_with(
        AxisScores {
            rhythm_similarity: 1.0,
            register_overlap: 1.0,
            density_ratio: 1.0,
            technique_overlap: 1.0,
        },
        2,
        2,
    );
    // weak aggregate = 0.25, strong = 1.0 → strong (index 1) ranks first.
    let order = rank_candidates(&[weak, strong], &relation_weights_v1());
    assert_eq!(order, vec![1, 0]);
}
