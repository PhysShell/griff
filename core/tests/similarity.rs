// TDD red phase: chunk similarity v2 — the gesture axes join the edge.
//
// v1 (decisions.log 2026-06-10, AudioMuse entry (a)) scored five axes over
// the S14 Phase-3 facts (structure metrics + tags). The gesture statistics
// landed as corpus schema v3 explicitly as "corpus facts and future S6
// constraint inputs; a similarity / scoring join is a follow-up"
// (decisions.log 2026-06-11). This is that join: the five *intensive*
// gesture distributions (mean burst length, mean rest length, rest grid
// share, modal landing share, final lengthening) become five more agreement
// axes under a uniform `similarity` v2 policy. Extensive facts (note /
// burst / rest counts, max burst) are deliberately not axes — they scale
// with chunk length, and a length echo would shadow the style facts.
//
// "Measured" now means structure *and* gesture: an absent fact is not a
// zero-similarity fact, and a pair scored on fewer axes would not be
// comparable under one policy. v1/v2 records re-curate to heal, the same
// convention v1 set for schema-v1 records.
//
// These reference `similarity_weights_v2` and the gesture axes, which do
// not exist yet, so the suite fails until the green step.
//
// v3 (the complexity join, schema v6): the five per-axis complexity facts
// that are not already on the edge — rhythmic / pitch / technical /
// harmonic / playability — become five more `1 − |Δ|` agreement axes under
// a uniform `similarity` v3 policy. `ComplexityProfile.structural` is
// deliberately not an axis: it is the same fact as
// `StructureMetrics.structural_complexity` (already `complexity_similarity`),
// and a duplicate axis would silently double-weight it — the
// `variation_score` rule. "Measured" now also requires the persisted
// profile; v≤5 records re-curate to heal. References
// `similarity_weights_v3` and the new axes, so the suite fails until the
// green step.
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
use griff_core::gesture::GestureStats;
use griff_core::scoring::Axes;
use griff_core::similarity::{
    find_similar_chunks, similarity_axes, similarity_weights_v3, SimilarityError,
    SIMILARITY_AXIS_LABELS,
};
use griff_core::structure::{ComplexityProfile, StructureMetrics};

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
        detected_subbar_period_ticks: None,
        repeatability_score: repeatability,
        variation_score: 1.0 - repeatability,
        loopability_score: loopability,
        structural_complexity: complexity,
    }
}

/// Gesture distributions with exactly-representable values; the extensive
/// counts are plausible fillers — the axes must not read them.
fn gesture(
    mean_burst_notes: f64,
    mean_rest_quarters: f64,
    rest_on_grid_share: f64,
    modal_landing_share: f64,
    mean_final_lengthening: f64,
) -> GestureStats {
    GestureStats {
        note_count: 12,
        burst_count: 3,
        mean_burst_notes,
        max_burst_notes: 6,
        rest_count: if mean_rest_quarters == 0.0 { 0 } else { 2 },
        mean_rest_quarters,
        rest_on_grid_share,
        modal_landing_share,
        mean_final_lengthening,
    }
}

/// The shared default gesture: identical on both sides of a pair, every
/// gesture axis reads 1.0, so structure/tag expectations stay undisturbed.
fn default_gesture() -> GestureStats {
    gesture(4.0, 1.0, 1.0, 0.5, 0.5)
}

/// The shared default complexity profile: identical on both sides of a
/// pair, every complexity axis reads 1.0, so structure/gesture/tag
/// expectations stay undisturbed.
const fn default_complexity() -> ComplexityProfile {
    ComplexityProfile {
        rhythmic: 0.5,
        pitch: 0.5,
        technical: 0.25,
        harmonic: 0.125,
        playability: 0.0,
        structural: 0.5,
    }
}

fn chunk(
    id: &str,
    structure: Option<StructureMetrics>,
    gesture: Option<GestureStats>,
    complexity: Option<ComplexityProfile>,
    tags: Vec<SwancoreTag>,
) -> ChunkMeta {
    ChunkMeta {
        id: ChunkId(id.to_owned()),
        title: id.to_owned(),
        source: SourceRef {
            filename: format!("{id}.mid"),
            format: SourceFormat::Midi,
            bar_range: Some((0, 4)),
            track_index: None,
            sha256: None,
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
        gesture,
        complexity,
        duplicate: None,
        style_cohort: None,
        ensemble: None,
        rights: None,
        created_at: "2026-06-10T00:00:00Z".to_owned(),
        updated_at: "2026-06-10T00:00:00Z".to_owned(),
    }
}

/// A fully measured chunk with the shared default gesture.
fn measured(id: &str, structure: StructureMetrics, tags: Vec<SwancoreTag>) -> ChunkMeta {
    chunk(
        id,
        Some(structure),
        Some(default_gesture()),
        Some(default_complexity()),
        tags,
    )
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
            "burst_length_similarity",
            "rest_length_similarity",
            "rest_grid_similarity",
            "modal_landing_similarity",
            "final_lengthening_similarity",
            "rhythmic_complexity_similarity",
            "pitch_complexity_similarity",
            "technical_complexity_similarity",
            "harmonic_complexity_similarity",
            "playability_complexity_similarity",
        ]
    );
}

#[test]
fn weights_v3_is_uniform_over_the_axes() {
    let policy = similarity_weights_v3();
    assert_eq!(policy.id, "similarity");
    assert_eq!(policy.version, 3);
    for label in SIMILARITY_AXIS_LABELS {
        assert_eq!(
            policy.weight(label),
            1.0 / 15.0,
            "uniform weight on {label}"
        );
    }
    assert_eq!(policy.weight("no_such_axis"), 0.0);
}

// ── per-axis facts ────────────────────────────────────────────────────────────

#[test]
fn identical_chunks_score_one_on_every_axis() {
    let m = metrics(Some(2), 0.75, 0.5, 0.25);
    let a = measured("a", m, vec![SwancoreTag::CleanRiff, SwancoreTag::Maj7]);
    let b = measured("b", m, vec![SwancoreTag::CleanRiff, SwancoreTag::Maj7]);

    let axes = similarity_axes(&a, &b).expect("both sides measured");
    assert_eq!(axes.len(), SIMILARITY_AXIS_LABELS.len());
    for label in SIMILARITY_AXIS_LABELS {
        assert_eq!(axis(&axes, label), 1.0, "identical chunks on {label}");
    }
}

#[test]
fn structure_unmeasured_side_yields_no_axes() {
    let full = measured("a", metrics(Some(1), 0.5, 0.5, 0.5), Vec::new());
    let no_structure = chunk(
        "b",
        None,
        Some(default_gesture()),
        Some(default_complexity()),
        Vec::new(),
    );

    assert!(similarity_axes(&full, &no_structure).is_none());
    assert!(similarity_axes(&no_structure, &full).is_none());
    assert!(similarity_axes(&no_structure, &no_structure).is_none());
}

#[test]
fn gesture_unmeasured_side_yields_no_axes() {
    // A schema-v2 record: measured structure, no gesture key. An absent fact
    // is not a zero-similarity fact, and a pair scored on fewer axes is not
    // comparable under one policy — so the pair has no axes at all.
    let full = measured("a", metrics(Some(1), 0.5, 0.5, 0.5), Vec::new());
    let v2_record = chunk(
        "b",
        Some(metrics(Some(1), 0.5, 0.5, 0.5)),
        None,
        Some(default_complexity()),
        Vec::new(),
    );

    assert!(similarity_axes(&full, &v2_record).is_none());
    assert!(similarity_axes(&v2_record, &full).is_none());
    assert!(similarity_axes(&v2_record, &v2_record).is_none());
}

#[test]
fn axes_are_symmetric() {
    let a = chunk(
        "a",
        Some(metrics(Some(2), 0.75, 1.0, 0.5)),
        Some(gesture(2.0, 0.0, 1.0, 0.75, 0.5)),
        Some(default_complexity()),
        vec![SwancoreTag::CleanRiff, SwancoreTag::PalmMute],
    );
    let b = chunk(
        "b",
        Some(metrics(Some(4), 0.25, 0.25, 0.25)),
        Some(gesture(4.0, 1.5, 0.25, 0.25, 0.25)),
        Some(default_complexity()),
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
    let a = measured("a", metrics(None, 0.0, 0.5, 1.0), Vec::new());
    let b = measured("b", metrics(None, 0.0, 0.5, 1.0), Vec::new());
    let axes = similarity_axes(&a, &b).expect("measured");
    assert_eq!(axis(&axes, "period_similarity"), 1.0);
}

#[test]
fn period_axis_one_sided_period_is_zero() {
    let periodic = measured("a", metrics(Some(2), 0.75, 0.5, 0.5), Vec::new());
    let through = measured("b", metrics(None, 0.0, 0.5, 0.5), Vec::new());
    let axes = similarity_axes(&periodic, &through).expect("measured");
    assert_eq!(axis(&axes, "period_similarity"), 0.0);
}

#[test]
fn period_axis_is_the_min_max_ratio() {
    let short = measured("a", metrics(Some(2), 0.75, 0.5, 0.5), Vec::new());
    let long = measured("b", metrics(Some(4), 0.75, 0.5, 0.5), Vec::new());
    let axes = similarity_axes(&short, &long).expect("measured");
    assert_eq!(axis(&axes, "period_similarity"), 0.5, "2 vs 4 bars");
}

#[test]
fn scalar_axes_measure_absolute_distance() {
    let a = measured("a", metrics(Some(1), 0.75, 1.0, 0.5), Vec::new());
    let b = measured("b", metrics(Some(1), 0.25, 0.25, 0.25), Vec::new());
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
    let two_tags = measured(
        "a",
        shared,
        vec![SwancoreTag::CleanRiff, SwancoreTag::PalmMute],
    );
    let four_tags = measured(
        "b",
        shared,
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
    let disjoint = measured("c", shared, vec![SwancoreTag::TappingPassage]);
    let disjoint_axes = similarity_axes(&two_tags, &disjoint).expect("measured");
    assert_eq!(axis(&disjoint_axes, "tag_similarity"), 0.0);

    // Duplicate tags count once: {X, X} vs {X} is identical as a set.
    let duplicated = measured(
        "d",
        shared,
        vec![SwancoreTag::CleanRiff, SwancoreTag::CleanRiff],
    );
    let single = measured("e", shared, vec![SwancoreTag::CleanRiff]);
    let dedup_axes = similarity_axes(&duplicated, &single).expect("measured");
    assert_eq!(axis(&dedup_axes, "tag_similarity"), 1.0);

    // Two untagged chunks agree (the empty-set convention of set_jaccard).
    let untagged_a = measured("f", shared, Vec::new());
    let untagged_b = measured("g", shared, Vec::new());
    let untagged_axes = similarity_axes(&untagged_a, &untagged_b).expect("measured");
    assert_eq!(axis(&untagged_axes, "tag_similarity"), 1.0);
}

// ── gesture axes ──────────────────────────────────────────────────────────────

#[test]
fn burst_length_axis_is_the_min_max_ratio() {
    let m = metrics(Some(1), 0.5, 0.5, 0.5);
    let short = chunk(
        "a",
        Some(m),
        Some(gesture(2.0, 1.0, 1.0, 0.5, 0.5)),
        Some(default_complexity()),
        Vec::new(),
    );
    let long = chunk(
        "b",
        Some(m),
        Some(gesture(4.0, 1.0, 1.0, 0.5, 0.5)),
        Some(default_complexity()),
        Vec::new(),
    );
    let axes = similarity_axes(&short, &long).expect("measured");
    assert_eq!(
        axis(&axes, "burst_length_similarity"),
        0.5,
        "2-note vs 4-note mean bursts"
    );
}

#[test]
fn rest_length_axis_two_restless_chunks_agree() {
    let m = metrics(Some(1), 0.5, 0.5, 0.5);
    // Restless chunks measure mean_rest_quarters = 0.0 and a vacuous grid
    // share of 1.0; two of them are the same gesture.
    let a = chunk(
        "a",
        Some(m),
        Some(gesture(4.0, 0.0, 1.0, 0.5, 0.5)),
        Some(default_complexity()),
        Vec::new(),
    );
    let b = chunk(
        "b",
        Some(m),
        Some(gesture(4.0, 0.0, 1.0, 0.5, 0.5)),
        Some(default_complexity()),
        Vec::new(),
    );
    let axes = similarity_axes(&a, &b).expect("measured");
    assert_eq!(axis(&axes, "rest_length_similarity"), 1.0);
}

#[test]
fn rest_length_axis_restless_vs_resting_is_zero() {
    let m = metrics(Some(1), 0.5, 0.5, 0.5);
    let restless = chunk(
        "a",
        Some(m),
        Some(gesture(4.0, 0.0, 1.0, 0.5, 0.5)),
        Some(default_complexity()),
        Vec::new(),
    );
    let resting = chunk(
        "b",
        Some(m),
        Some(gesture(4.0, 1.5, 1.0, 0.5, 0.5)),
        Some(default_complexity()),
        Vec::new(),
    );
    let axes = similarity_axes(&restless, &resting).expect("measured");
    assert_eq!(
        axis(&axes, "rest_length_similarity"),
        0.0,
        "wall-to-wall vs gestured writing is fully dissimilar on rests"
    );
}

#[test]
fn rest_length_axis_is_the_min_max_ratio() {
    let m = metrics(Some(1), 0.5, 0.5, 0.5);
    let short = chunk(
        "a",
        Some(m),
        Some(gesture(4.0, 1.0, 1.0, 0.5, 0.5)),
        Some(default_complexity()),
        Vec::new(),
    );
    let long = chunk(
        "b",
        Some(m),
        Some(gesture(4.0, 2.0, 1.0, 0.5, 0.5)),
        Some(default_complexity()),
        Vec::new(),
    );
    let axes = similarity_axes(&short, &long).expect("measured");
    assert_eq!(
        axis(&axes, "rest_length_similarity"),
        0.5,
        "1-quarter vs 2-quarter mean rests"
    );
}

#[test]
fn gesture_share_axes_measure_absolute_distance() {
    let m = metrics(Some(1), 0.5, 0.5, 0.5);
    let a = chunk(
        "a",
        Some(m),
        Some(gesture(4.0, 1.0, 0.75, 1.0, 0.5)),
        Some(default_complexity()),
        Vec::new(),
    );
    let b = chunk(
        "b",
        Some(m),
        Some(gesture(4.0, 1.0, 0.25, 0.25, 0.25)),
        Some(default_complexity()),
        Vec::new(),
    );
    let axes = similarity_axes(&a, &b).expect("measured");
    assert_eq!(
        axis(&axes, "rest_grid_similarity"),
        0.5,
        "1 − |0.75 − 0.25|"
    );
    assert_eq!(
        axis(&axes, "modal_landing_similarity"),
        0.25,
        "1 − |1.0 − 0.25|"
    );
    assert_eq!(
        axis(&axes, "final_lengthening_similarity"),
        0.75,
        "1 − |0.5 − 0.25|"
    );
}

// ── retrieval ─────────────────────────────────────────────────────────────────

#[test]
fn find_similar_requires_a_measured_query() {
    let candidates = vec![measured("a", metrics(Some(1), 0.5, 0.5, 0.5), Vec::new())];
    let policy = similarity_weights_v3();

    // No structure at all (schema v1).
    let v1_query = chunk("q", None, None, None, Vec::new());
    assert_eq!(
        find_similar_chunks(&v1_query, &candidates, &policy).unwrap_err(),
        SimilarityError::QueryUnmeasured
    );

    // Structure but no gesture (schema v2) is still unmeasured for this edge.
    let v2_query = chunk(
        "q",
        Some(metrics(Some(1), 0.5, 0.5, 0.5)),
        None,
        None,
        Vec::new(),
    );
    assert_eq!(
        find_similar_chunks(&v2_query, &candidates, &policy).unwrap_err(),
        SimilarityError::QueryUnmeasured
    );
}

#[test]
fn find_similar_excludes_the_query_itself() {
    let m = metrics(Some(1), 0.5, 0.5, 0.5);
    let query = measured("q", m, Vec::new());
    let candidates = vec![measured("q", m, Vec::new()), measured("a", m, Vec::new())];
    let policy = similarity_weights_v3();

    let ranked = find_similar_chunks(&query, &candidates, &policy).expect("measured query");
    assert_eq!(ranked.len(), 1);
    assert_eq!(ranked[0].value, ChunkId("a".to_owned()));
}

#[test]
fn find_similar_skips_unmeasured_candidates() {
    let m = metrics(Some(1), 0.5, 0.5, 0.5);
    let query = measured("q", m, Vec::new());
    let candidates = vec![
        chunk("v1_record", None, None, None, Vec::new()),
        chunk("v2_record", Some(m), None, None, Vec::new()),
        chunk(
            "v5_record",
            Some(m),
            Some(default_gesture()),
            None,
            Vec::new(),
        ),
        measured("measured", m, Vec::new()),
    ];
    let policy = similarity_weights_v3();

    let ranked = find_similar_chunks(&query, &candidates, &policy).expect("measured query");
    assert_eq!(
        ranked.len(),
        1,
        "v1, v2, and v5 records sit out until re-curated"
    );
    assert_eq!(ranked[0].value, ChunkId("measured".to_owned()));
}

#[test]
fn find_similar_ranks_closer_chunks_first() {
    let query = chunk(
        "q",
        Some(metrics(Some(2), 0.75, 0.75, 0.25)),
        Some(gesture(4.0, 1.0, 1.0, 0.5, 0.5)),
        Some(default_complexity()),
        vec![SwancoreTag::CleanRiff, SwancoreTag::PalmMute],
    );
    let candidates = vec![
        // Far: different period shape, distant scalars, disjoint tags, an
        // opposite gesture (long rests off the grid).
        chunk(
            "far",
            Some(metrics(None, 0.0, 0.0, 1.0)),
            Some(gesture(1.0, 4.0, 0.0, 1.0, 1.0)),
            Some(ComplexityProfile {
                rhythmic: 1.0,
                pitch: 0.0,
                technical: 1.0,
                harmonic: 1.0,
                playability: 1.0,
                structural: 1.0,
            }),
            vec![SwancoreTag::TappingPassage],
        ),
        // Near: identical metrics, tags, and gesture.
        chunk(
            "near",
            Some(metrics(Some(2), 0.75, 0.75, 0.25)),
            Some(gesture(4.0, 1.0, 1.0, 0.5, 0.5)),
            Some(default_complexity()),
            vec![SwancoreTag::CleanRiff, SwancoreTag::PalmMute],
        ),
        // Middle: same period, scalars off by 0.25, half-overlapping tags,
        // a same-shape gesture at half the burst length.
        chunk(
            "middle",
            Some(metrics(Some(2), 0.5, 0.5, 0.5)),
            Some(gesture(2.0, 1.0, 1.0, 0.5, 0.5)),
            Some(default_complexity()),
            vec![SwancoreTag::CleanRiff],
        ),
    ];
    let policy = similarity_weights_v3();

    let ranked = find_similar_chunks(&query, &candidates, &policy).expect("measured query");
    let ids: Vec<&str> = ranked.iter().map(|s| s.value.0.as_str()).collect();
    assert_eq!(ids, ["near", "middle", "far"]);
    assert!(
        (ranked[0].aggregate() - 1.0).abs() < 1e-12,
        "identical chunk aggregates to 1 (uniform weights over all-1 axes)"
    );
    assert!(ranked[1].aggregate() > ranked[2].aggregate());
}

#[test]
fn find_similar_ties_break_by_candidate_order() {
    let m = metrics(Some(1), 0.5, 0.5, 0.5);
    let query = measured("q", m, Vec::new());
    let candidates = vec![
        measured("first", m, Vec::new()),
        measured("second", m, Vec::new()),
    ];
    let policy = similarity_weights_v3();

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
    let query = measured("q", m, Vec::new());
    let candidates = vec![measured("a", m, Vec::new())];
    let policy = similarity_weights_v3();

    let ranked = find_similar_chunks(&query, &candidates, &policy).expect("measured query");
    let scored = &ranked[0];
    assert_eq!(scored.provenance.policy_id, "similarity");
    assert_eq!(scored.provenance.policy_version, 3);
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

// ── complexity axes (similarity v3, corpus schema v6) ─────────────────────────

#[test]
fn complexity_unmeasured_side_yields_no_axes() {
    // A schema-v5 record: measured structure and gesture, no complexity
    // profile. The all-or-nothing measured rule extends to the new facts.
    let full = measured("a", metrics(Some(1), 0.5, 0.5, 0.5), Vec::new());
    let v5_record = chunk(
        "b",
        Some(metrics(Some(1), 0.5, 0.5, 0.5)),
        Some(default_gesture()),
        None,
        Vec::new(),
    );

    assert!(similarity_axes(&full, &v5_record).is_none());
    assert!(similarity_axes(&v5_record, &full).is_none());
    assert!(similarity_axes(&v5_record, &v5_record).is_none());
}

#[test]
fn complexity_axes_measure_absolute_distance() {
    let m = metrics(Some(1), 0.5, 0.5, 0.5);
    let a = chunk(
        "a",
        Some(m),
        Some(default_gesture()),
        Some(ComplexityProfile {
            rhythmic: 0.5,
            pitch: 1.0,
            technical: 0.75,
            harmonic: 0.625,
            playability: 1.0,
            structural: 0.5,
        }),
        Vec::new(),
    );
    let b = chunk(
        "b",
        Some(m),
        Some(default_gesture()),
        Some(ComplexityProfile {
            rhythmic: 0.25,
            pitch: 0.25,
            technical: 0.75,
            harmonic: 0.125,
            playability: 0.0,
            structural: 0.5,
        }),
        Vec::new(),
    );
    let axes = similarity_axes(&a, &b).expect("measured");
    assert_eq!(
        axis(&axes, "rhythmic_complexity_similarity"),
        0.75,
        "1 − |0.5 − 0.25|"
    );
    assert_eq!(
        axis(&axes, "pitch_complexity_similarity"),
        0.25,
        "1 − |1.0 − 0.25|"
    );
    assert_eq!(
        axis(&axes, "technical_complexity_similarity"),
        1.0,
        "identical technical shares"
    );
    assert_eq!(
        axis(&axes, "harmonic_complexity_similarity"),
        0.5,
        "1 − |0.625 − 0.125|"
    );
    assert_eq!(
        axis(&axes, "playability_complexity_similarity"),
        0.0,
        "open-position line vs an unreachable one"
    );
}

#[test]
fn find_similar_rejects_a_query_without_complexity() {
    // A v5 query: structure and gesture measured, no complexity profile —
    // unmeasured for this edge until re-curated.
    let candidates = vec![measured("a", metrics(Some(1), 0.5, 0.5, 0.5), Vec::new())];
    let policy = similarity_weights_v3();
    let v5_query = chunk(
        "q",
        Some(metrics(Some(1), 0.5, 0.5, 0.5)),
        Some(default_gesture()),
        None,
        Vec::new(),
    );
    assert_eq!(
        find_similar_chunks(&v5_query, &candidates, &policy).unwrap_err(),
        SimilarityError::QueryUnmeasured
    );
}
