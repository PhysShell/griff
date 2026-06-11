// TDD red phase: chunk similarity v1 — the first S7 edge over the corpus
// axes persisted by S14 Phase 3 (decisions.log 2026-06-10, AudioMuse entry
// (a): brute-force per-axis similarity with an ADR-0017 rationale; no ANN at
// micro-corpus scale).
//
// These reference `griff_core::similarity`, which does not exist yet, so the
// suite fails to compile until the green step.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::missing_assert_message,
    clippy::float_cmp,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]

use griff_core::corpus::{ChunkId, ChunkMeta, SourceFormat, SourceRef, SwancoreTag};
use griff_core::scoring::Axes;
use griff_core::similarity::{
    find_similar_chunks, similarity_axes, similarity_weights_v1, SimilarityError,
    SIMILARITY_AXIS_LABELS,
};
use griff_core::structure::StructureMetrics;

// ── helpers ───────────────────────────────────────────────────────────────────

/// Exactly-representable f64 values so axis arithmetic compares equal to
/// literals.
const fn metrics(
    period_bars: Option<usize>,
    repeatability: f64,
    loopability: f64,
    complexity: f64,
) -> StructureMetrics {
    StructureMetrics {
        bar_count: 4,
        detected_pattern_period_bars: period_bars,
        detected_pattern_period_ticks: match period_bars {
            Some(p) => Some(1920 * p as u32),
            None => None,
        },
        repeatability_score: repeatability,
        variation_score: 1.0 - repeatability,
        loopability_score: loopability,
        structural_complexity: complexity,
    }
}

fn chunk(id: &str, structure: Option<StructureMetrics>, tags: Vec<SwancoreTag>) -> ChunkMeta {
    ChunkMeta {
        id: ChunkId(id.to_owned()),
        title: id.to_owned(),
        source: SourceRef {
            filename: format!("{id}.mid"),
            format: SourceFormat::Midi,
            bar_range: Some((0, 4)),
        },
        tempo_bpm: 140.0,
        ticks_per_quarter: 960,
        time_signature: (4, 4),
        tuning: "standard_e".to_owned(),
        tags,
        boundaries: Vec::new(),
        techniques: Vec::new(),
        quality_flags: Vec::new(),
        reviewer: None,
        structure,
        gesture: None,
        created_at: "2026-06-10T00:00:00Z".to_owned(),
        updated_at: "2026-06-10T00:00:00Z".to_owned(),
    }
}

fn axis(axes: &Axes, label: &str) -> f64 {
    axes.get(label)
        .unwrap_or_else(|| panic!("axis {label} missing"))
}

// ── axis vocabulary and policy ────────────────────────────────────────────────

#[test]
fn axis_labels_are_canonical() {
    assert_eq!(
        SIMILARITY_AXIS_LABELS,
        [
            "period_similarity",
            "repeatability_similarity",
            "loopability_similarity",
            "complexity_similarity",
            "tag_similarity",
        ]
    );
}

#[test]
fn weights_v1_is_uniform_over_the_axes() {
    let policy = similarity_weights_v1();
    assert_eq!(policy.id, "similarity");
    assert_eq!(policy.version, 1);
    for label in SIMILARITY_AXIS_LABELS {
        assert_eq!(policy.weight(label), 0.2, "uniform weight on {label}");
    }
    assert_eq!(policy.weight("no_such_axis"), 0.0);
}

// ── per-axis facts ────────────────────────────────────────────────────────────

#[test]
fn identical_chunks_score_one_on_every_axis() {
    let m = metrics(Some(2), 0.75, 0.5, 0.25);
    let a = chunk(
        "a",
        Some(m),
        vec![SwancoreTag::CleanRiff, SwancoreTag::Maj7],
    );
    let b = chunk(
        "b",
        Some(m),
        vec![SwancoreTag::CleanRiff, SwancoreTag::Maj7],
    );

    let axes = similarity_axes(&a, &b).expect("both sides measured");
    assert_eq!(axes.len(), SIMILARITY_AXIS_LABELS.len());
    for label in SIMILARITY_AXIS_LABELS {
        assert_eq!(axis(&axes, label), 1.0, "identical chunks on {label}");
    }
}

#[test]
fn unmeasured_side_yields_no_axes() {
    let measured = chunk("a", Some(metrics(Some(1), 0.5, 0.5, 0.5)), Vec::new());
    let unmeasured = chunk("b", None, Vec::new());

    assert!(similarity_axes(&measured, &unmeasured).is_none());
    assert!(similarity_axes(&unmeasured, &measured).is_none());
    assert!(similarity_axes(&unmeasured, &unmeasured).is_none());
}

#[test]
fn axes_are_symmetric() {
    let a = chunk(
        "a",
        Some(metrics(Some(2), 0.75, 1.0, 0.5)),
        vec![SwancoreTag::CleanRiff, SwancoreTag::PalmMute],
    );
    let b = chunk(
        "b",
        Some(metrics(Some(4), 0.25, 0.25, 0.25)),
        vec![SwancoreTag::PalmMute, SwancoreTag::Syncopated],
    );

    let ab = similarity_axes(&a, &b).expect("measured");
    let ba = similarity_axes(&b, &a).expect("measured");
    for label in SIMILARITY_AXIS_LABELS {
        assert_eq!(axis(&ab, label), axis(&ba, label), "symmetry on {label}");
    }
}

#[test]
fn period_axis_through_composed_pair_agrees() {
    let a = chunk("a", Some(metrics(None, 0.0, 0.5, 1.0)), Vec::new());
    let b = chunk("b", Some(metrics(None, 0.0, 0.5, 1.0)), Vec::new());
    let axes = similarity_axes(&a, &b).expect("measured");
    assert_eq!(axis(&axes, "period_similarity"), 1.0);
}

#[test]
fn period_axis_one_sided_period_is_zero() {
    let periodic = chunk("a", Some(metrics(Some(2), 0.75, 0.5, 0.5)), Vec::new());
    let through = chunk("b", Some(metrics(None, 0.0, 0.5, 0.5)), Vec::new());
    let axes = similarity_axes(&periodic, &through).expect("measured");
    assert_eq!(axis(&axes, "period_similarity"), 0.0);
}

#[test]
fn period_axis_is_the_min_max_ratio() {
    let short = chunk("a", Some(metrics(Some(2), 0.75, 0.5, 0.5)), Vec::new());
    let long = chunk("b", Some(metrics(Some(4), 0.75, 0.5, 0.5)), Vec::new());
    let axes = similarity_axes(&short, &long).expect("measured");
    assert_eq!(axis(&axes, "period_similarity"), 0.5, "2 vs 4 bars");
}

#[test]
fn scalar_axes_measure_absolute_distance() {
    let a = chunk("a", Some(metrics(Some(1), 0.75, 1.0, 0.5)), Vec::new());
    let b = chunk("b", Some(metrics(Some(1), 0.25, 0.25, 0.25)), Vec::new());
    let axes = similarity_axes(&a, &b).expect("measured");
    assert_eq!(
        axis(&axes, "repeatability_similarity"),
        0.5,
        "1 − |0.75 − 0.25|"
    );
    assert_eq!(
        axis(&axes, "loopability_similarity"),
        0.25,
        "1 − |1.0 − 0.25|"
    );
    assert_eq!(
        axis(&axes, "complexity_similarity"),
        0.75,
        "1 − |0.5 − 0.25|"
    );
}

#[test]
fn tag_axis_is_jaccard_over_tag_sets() {
    let shared = metrics(Some(1), 0.5, 0.5, 0.5);
    // Overlap 2, union 4 → 0.5 (exactly representable).
    let two_tags = chunk(
        "a",
        Some(shared),
        vec![SwancoreTag::CleanRiff, SwancoreTag::PalmMute],
    );
    let four_tags = chunk(
        "b",
        Some(shared),
        vec![
            SwancoreTag::CleanRiff,
            SwancoreTag::PalmMute,
            SwancoreTag::Syncopated,
            SwancoreTag::Maj7,
        ],
    );
    let overlap_axes = similarity_axes(&two_tags, &four_tags).expect("measured");
    assert_eq!(axis(&overlap_axes, "tag_similarity"), 0.5);

    // Disjoint sets → 0.0.
    let disjoint = chunk("c", Some(shared), vec![SwancoreTag::TappingPassage]);
    let disjoint_axes = similarity_axes(&two_tags, &disjoint).expect("measured");
    assert_eq!(axis(&disjoint_axes, "tag_similarity"), 0.0);

    // Duplicate tags count once: {X, X} vs {X} is identical as a set.
    let duplicated = chunk(
        "d",
        Some(shared),
        vec![SwancoreTag::CleanRiff, SwancoreTag::CleanRiff],
    );
    let single = chunk("e", Some(shared), vec![SwancoreTag::CleanRiff]);
    let dedup_axes = similarity_axes(&duplicated, &single).expect("measured");
    assert_eq!(axis(&dedup_axes, "tag_similarity"), 1.0);

    // Two untagged chunks agree (the empty-set convention of set_jaccard).
    let untagged_a = chunk("f", Some(shared), Vec::new());
    let untagged_b = chunk("g", Some(shared), Vec::new());
    let untagged_axes = similarity_axes(&untagged_a, &untagged_b).expect("measured");
    assert_eq!(axis(&untagged_axes, "tag_similarity"), 1.0);
}

// ── retrieval ─────────────────────────────────────────────────────────────────

#[test]
fn find_similar_requires_a_measured_query() {
    let query = chunk("q", None, Vec::new());
    let candidates = vec![chunk(
        "a",
        Some(metrics(Some(1), 0.5, 0.5, 0.5)),
        Vec::new(),
    )];
    let policy = similarity_weights_v1();

    assert_eq!(
        find_similar_chunks(&query, &candidates, &policy).unwrap_err(),
        SimilarityError::QueryUnmeasured
    );
}

#[test]
fn find_similar_excludes_the_query_itself() {
    let m = metrics(Some(1), 0.5, 0.5, 0.5);
    let query = chunk("q", Some(m), Vec::new());
    let candidates = vec![
        chunk("q", Some(m), Vec::new()),
        chunk("a", Some(m), Vec::new()),
    ];
    let policy = similarity_weights_v1();

    let ranked = find_similar_chunks(&query, &candidates, &policy).expect("measured query");
    assert_eq!(ranked.len(), 1);
    assert_eq!(ranked[0].value, ChunkId("a".to_owned()));
}

#[test]
fn find_similar_skips_unmeasured_candidates() {
    let m = metrics(Some(1), 0.5, 0.5, 0.5);
    let query = chunk("q", Some(m), Vec::new());
    let candidates = vec![
        chunk("v1_record", None, Vec::new()),
        chunk("measured", Some(m), Vec::new()),
    ];
    let policy = similarity_weights_v1();

    let ranked = find_similar_chunks(&query, &candidates, &policy).expect("measured query");
    assert_eq!(ranked.len(), 1);
    assert_eq!(ranked[0].value, ChunkId("measured".to_owned()));
}

#[test]
fn find_similar_ranks_closer_chunks_first() {
    let query = chunk(
        "q",
        Some(metrics(Some(2), 0.75, 0.75, 0.25)),
        vec![SwancoreTag::CleanRiff, SwancoreTag::PalmMute],
    );
    let candidates = vec![
        // Far: different period shape, distant scalars, disjoint tags.
        chunk(
            "far",
            Some(metrics(None, 0.0, 0.0, 1.0)),
            vec![SwancoreTag::TappingPassage],
        ),
        // Near: identical metrics and tags.
        chunk(
            "near",
            Some(metrics(Some(2), 0.75, 0.75, 0.25)),
            vec![SwancoreTag::CleanRiff, SwancoreTag::PalmMute],
        ),
        // Middle: same period, scalars off by 0.25, half-overlapping tags.
        chunk(
            "middle",
            Some(metrics(Some(2), 0.5, 0.5, 0.5)),
            vec![SwancoreTag::CleanRiff],
        ),
    ];
    let policy = similarity_weights_v1();

    let ranked = find_similar_chunks(&query, &candidates, &policy).expect("measured query");
    let ids: Vec<&str> = ranked.iter().map(|s| s.value.0.as_str()).collect();
    assert_eq!(ids, ["near", "middle", "far"]);
    assert_eq!(
        ranked[0].aggregate(),
        1.0,
        "identical chunk aggregates to 1"
    );
    assert!(ranked[1].aggregate() > ranked[2].aggregate());
}

#[test]
fn find_similar_ties_break_by_candidate_order() {
    let m = metrics(Some(1), 0.5, 0.5, 0.5);
    let query = chunk("q", Some(m), Vec::new());
    let candidates = vec![
        chunk("first", Some(m), Vec::new()),
        chunk("second", Some(m), Vec::new()),
    ];
    let policy = similarity_weights_v1();

    let ranked = find_similar_chunks(&query, &candidates, &policy).expect("measured query");
    let ids: Vec<&str> = ranked.iter().map(|s| s.value.0.as_str()).collect();
    assert_eq!(
        ids,
        ["first", "second"],
        "equal aggregates keep input order"
    );
}

#[test]
fn scored_envelope_carries_policy_provenance() {
    let m = metrics(Some(1), 0.5, 0.5, 0.5);
    let query = chunk("q", Some(m), Vec::new());
    let candidates = vec![chunk("a", Some(m), Vec::new())];
    let policy = similarity_weights_v1();

    let ranked = find_similar_chunks(&query, &candidates, &policy).expect("measured query");
    let scored = &ranked[0];
    assert_eq!(scored.provenance.policy_id, "similarity");
    assert_eq!(scored.provenance.policy_version, 1);
    assert_eq!(scored.provenance.seed, None, "purely analytic score");

    let labels: Vec<&str> = scored.axes.iter().map(|a| a.label).collect();
    assert_eq!(labels, SIMILARITY_AXIS_LABELS, "axes in canonical order");

    let contributions: f64 = scored
        .rationale
        .entries()
        .iter()
        .map(|e| e.contribution)
        .sum();
    assert_eq!(scored.aggregate(), contributions, "aggregate is derived");
}
