// TDD red phase for S5: corpus schema round-trip tests.
// These fail to compile until `core/src/corpus.rs` is implemented.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::missing_assert_message
)]

use griff_core::complement::AxisScores;
use griff_core::corpus::{
    Acquisition, BoundaryEntry, ChunkId, ChunkMeta, CorpusManifest, EnsembleGroup, EnsembleRef,
    PairRelation, QualityFlag, ReviewerDecision, RightsInfo, RightsStatus, SourceFormat, SourceRef,
    StyleCohort, SwancoreTag, SCHEMA_VERSION,
};
use griff_core::gesture::GestureStats;
use griff_core::novelty::PhraseDuplicate;
use griff_core::structure::{ComplexityProfile, StructureMetrics};
use proptest::{collection::vec as prop_vec, option::of as prop_opt, prelude::*};

// ── helpers ────────────────────────────────────────────────────────────────────

/// Exactly-representable f64 values keep JSON round-trips byte-identical.
const fn sample_metrics() -> StructureMetrics {
    StructureMetrics {
        bar_count: 4,
        detected_pattern_period_bars: Some(1),
        detected_pattern_period_ticks: Some(1920),
        detected_subbar_period_ticks: Some(960),
        repeatability_score: 0.9375,
        variation_score: 0.0625,
        loopability_score: 0.875,
        structural_complexity: 0.25,
    }
}

/// Exactly-representable f64 values keep JSON round-trips byte-identical.
const fn sample_complexity() -> ComplexityProfile {
    ComplexityProfile {
        rhythmic: 0.25,
        pitch: 0.5,
        technical: 0.0,
        harmonic: 0.125,
        playability: 0.0625,
        structural: 0.5,
    }
}

/// Exactly-representable f64 values keep JSON round-trips byte-identical.
const fn sample_gesture() -> GestureStats {
    GestureStats {
        note_count: 16,
        burst_count: 2,
        mean_burst_notes: 8.0,
        max_burst_notes: 8,
        rest_count: 2,
        mean_rest_quarters: 1.5,
        rest_on_grid_share: 1.0,
        modal_landing_share: 0.5,
        mean_final_lengthening: 0.5,
    }
}

fn minimal_chunk() -> ChunkMeta {
    ChunkMeta {
        id: ChunkId("test_001".to_owned()),
        title: "Test Chunk".to_owned(),
        source: SourceRef {
            filename: "test.mid".to_owned(),
            format: SourceFormat::Midi,
            bar_range: Some((0, 4)),
        },
        tempo_bpm: 140.0,
        ticks_per_quarter: 960,
        time_signature: (4, 4),
        tuning: "standard_e".to_owned(),
        tags: vec![SwancoreTag::CleanRiff, SwancoreTag::SyncopatedRiff],
        boundaries: vec![BoundaryEntry {
            start_tick: 0,
            end_tick: 3840,
            score: 0.75,
        }],
        techniques: vec!["hammer_on".to_owned()],
        quality_flags: vec![QualityFlag::Clean],
        reviewer: Some(ReviewerDecision::Accepted),
        structure: None,
        gesture: None,
        complexity: None,
        duplicate: None,
        style_cohort: None,
        ensemble: None,
        rights: None,
        created_at: "2026-05-20T00:00:00Z".to_owned(),
        updated_at: "2026-05-20T00:00:00Z".to_owned(),
    }
}

// ── schema v8: near-duplicate link (#76) ──────────────────────────────────────

#[test]
fn schema_version_is_8() {
    assert_eq!(
        SCHEMA_VERSION, 9,
        "the persisted near-duplicate link bumps the corpus schema"
    );
}

#[test]
fn chunk_meta_persists_and_skips_the_near_duplicate_link() {
    // Absent by default → the key is skipped (not written as null), so pre-v8
    // records round-trip byte-identically.
    let plain_json = serde_json::to_string(&minimal_chunk()).expect("serialize");
    assert!(
        !plain_json.contains("duplicate"),
        "an unset duplicate link is skipped, not null: {plain_json}"
    );

    // Present → a nested object that round-trips losslessly.
    let mut flagged = minimal_chunk();
    flagged.duplicate = Some(PhraseDuplicate {
        of: 1,
        quote_share: 0.9375,
    });
    let json = serde_json::to_string(&flagged).expect("serialize");
    assert!(json.contains("\"duplicate\""), "{json}");
    assert!(json.contains("\"of\":1"), "{json}");
    let back: ChunkMeta = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(
        back, flagged,
        "the near-duplicate link round-trips losslessly"
    );
}

// ── schema v7: rights + provenance (decisions 2026-06-12) ─────────────────────

/// A representative rights record (the common scraped-community-tab case).
fn sample_rights() -> RightsInfo {
    RightsInfo {
        rights_status: RightsStatus::CopyrightedComposition,
        acquisition: Acquisition::CommunityTabSite,
        redistributable: false,
        notes: "ultimate-guitar.com, 2026-06-12".to_owned(),
    }
}

#[test]
fn chunk_meta_with_rights_roundtrips() {
    let mut meta = minimal_chunk();
    meta.rights = Some(sample_rights());

    let json = serde_json::to_string(&meta).expect("serialize");
    assert!(
        json.contains("\"rights\""),
        "v7 records carry the rights key"
    );
    assert!(
        json.contains("\"copyrighted_composition\"") && json.contains("\"community_tab_site\""),
        "rights enums serialize snake_case: {json}"
    );
    let back: ChunkMeta = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.rights, Some(sample_rights()));
    let json2 = serde_json::to_string(&back).expect("re-serialize");
    assert_eq!(json, json2, "JSON round-trip must be byte-identical");
}

#[test]
fn pre_v7_record_without_rights_loads_as_none() {
    // A pre-v7 record has no rights key; it parses as None and re-serializes
    // without inventing the key.
    let old_json = serde_json::to_string(&minimal_chunk()).expect("serialize");
    assert!(
        !old_json.contains("\"rights\""),
        "an absent rights record must not introduce a key: {old_json}"
    );

    let back: ChunkMeta = serde_json::from_str(&old_json).expect("pre-v7 record must parse");
    assert!(back.rights.is_none());
    let json2 = serde_json::to_string(&back).expect("re-serialize");
    assert_eq!(old_json, json2, "JSON round-trip must be byte-identical");
}

#[test]
fn all_rights_statuses_roundtrip() {
    for status in [
        RightsStatus::PublicDomain,
        RightsStatus::CcBy,
        RightsStatus::CcBySa,
        RightsStatus::CopyrightedComposition,
        RightsStatus::Unknown,
    ] {
        let json = serde_json::to_string(&status).expect("serialize");
        let back: RightsStatus = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(status, back);
    }
}

#[test]
fn all_acquisitions_roundtrip() {
    for acq in [
        Acquisition::CommunityTabSite,
        Acquisition::PurchasedOfficial,
        Acquisition::SelfTranscribed,
        Acquisition::OmrFromScan,
        Acquisition::ArtistProvided,
    ] {
        let json = serde_json::to_string(&acq).expect("serialize");
        let back: Acquisition = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(acq, back);
    }
}

// ── schema v6: the per-axis complexity profile (S14) ──────────────────────────

#[test]
fn chunk_meta_with_complexity_roundtrips() {
    let mut meta = minimal_chunk();
    meta.complexity = Some(sample_complexity());

    let json = serde_json::to_string(&meta).expect("serialize");
    assert!(
        json.contains("\"complexity\""),
        "v6 records carry the complexity key"
    );
    let back: ChunkMeta = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.complexity, Some(sample_complexity()));
    let json2 = serde_json::to_string(&back).expect("re-serialize");
    assert_eq!(json, json2, "JSON round-trip must be byte-identical");
}

#[test]
fn pre_v6_record_without_complexity_loads_as_none() {
    // A pre-v6 record has no complexity key; it parses as None and
    // re-serializes without inventing the key.
    let old_json = serde_json::to_string(&minimal_chunk()).expect("serialize");
    assert!(
        !old_json.contains("\"complexity\""),
        "an absent profile must not introduce a key: {old_json}"
    );

    let back: ChunkMeta = serde_json::from_str(&old_json).expect("pre-v6 record must parse");
    assert!(back.complexity.is_none());
    let json2 = serde_json::to_string(&back).expect("re-serialize");
    assert_eq!(old_json, json2, "JSON round-trip must be byte-identical");
}

// ── schema v5: sub-bar period (S14 sub-bar detection) ─────────────────────────

#[test]
fn pre_v5_structure_without_subbar_key_loads_as_none() {
    // A pre-v5 record's structure block has no sub-bar key; it parses as
    // None and re-serializes without inventing the key.
    let mut metrics = sample_metrics();
    metrics.detected_subbar_period_ticks = None;
    let mut meta = minimal_chunk();
    meta.structure = Some(metrics);
    let old_json = serde_json::to_string(&meta).expect("serialize");
    assert!(
        !old_json.contains("detected_subbar_period_ticks"),
        "an absent sub-bar period must not introduce a key: {old_json}"
    );

    let back: ChunkMeta = serde_json::from_str(&old_json).expect("pre-v5 record must parse");
    assert_eq!(back.structure, Some(metrics));
    let json2 = serde_json::to_string(&back).expect("re-serialize");
    assert_eq!(old_json, json2, "JSON round-trip must be byte-identical");
}

#[test]
fn structure_with_subbar_period_roundtrips() {
    let mut meta = minimal_chunk();
    meta.structure = Some(sample_metrics());

    let json = serde_json::to_string(&meta).expect("serialize");
    assert!(
        json.contains("\"detected_subbar_period_ticks\":960"),
        "v5 records carry the sub-bar period: {json}"
    );
    let back: ChunkMeta = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.structure, Some(sample_metrics()));
    let json2 = serde_json::to_string(&back).expect("re-serialize");
    assert_eq!(json, json2, "JSON round-trip must be byte-identical");
}

// ── schema v4: style cohort + ensemble groups (decisions 2026-06-11) ──────────

#[test]
fn chunk_meta_with_cohort_and_ensemble_roundtrips() {
    let mut meta = minimal_chunk();
    meta.style_cohort = Some(StyleCohort::Adjacent);
    meta.ensemble = Some(EnsembleRef {
        group_id: "dgd_042".to_owned(),
        part_index: 1,
    });

    let json = serde_json::to_string(&meta).expect("serialize");
    assert!(
        json.contains("\"adjacent\""),
        "cohort serializes snake_case: {json}"
    );
    assert!(json.contains("\"ensemble\""), "v4 records carry the link");
    let back: ChunkMeta = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.style_cohort, Some(StyleCohort::Adjacent));
    assert_eq!(
        back.ensemble,
        Some(EnsembleRef {
            group_id: "dgd_042".to_owned(),
            part_index: 1,
        })
    );
    let json2 = serde_json::to_string(&back).expect("re-serialize");
    assert_eq!(json, json2, "JSON round-trip must be byte-identical");
}

#[test]
fn absent_v4_fields_serialize_without_keys() {
    // A record predating v4 stays byte-identical: neither key is written.
    let json = serde_json::to_string(&minimal_chunk()).expect("serialize");
    assert!(
        !json.contains("\"style_cohort\""),
        "absent cohort must not introduce a key: {json}"
    );
    assert!(
        !json.contains("\"ensemble\""),
        "absent link must not introduce a key: {json}"
    );
}

#[test]
fn both_cohorts_roundtrip() {
    for cohort in [StyleCohort::Core, StyleCohort::Adjacent] {
        let json = serde_json::to_string(&cohort).expect("serialize");
        let back: StyleCohort = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(cohort, back);
    }
}

#[test]
fn manifest_groups_roundtrip() {
    // Exactly-representable axis values keep the round-trip byte-identical.
    let group = EnsembleGroup {
        id: "dgd_042".to_owned(),
        members: vec![
            ChunkId("dgd_042_p0".to_owned()),
            ChunkId("dgd_042_p1".to_owned()),
        ],
        relations: vec![PairRelation {
            parts: (0, 1),
            axes: AxisScores {
                rhythm_similarity: 0.5,
                register_overlap: 0.0,
                density_ratio: 0.5,
                technique_overlap: 1.0,
            },
        }],
    };
    let manifest = CorpusManifest {
        schema_version: SCHEMA_VERSION,
        chunks: vec![minimal_chunk()],
        groups: vec![group.clone()],
    };

    let json = serde_json::to_string(&manifest).expect("serialize");
    let back: CorpusManifest = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.groups, vec![group]);
    let json2 = serde_json::to_string(&back).expect("re-serialize");
    assert_eq!(json, json2, "manifest round-trip must be byte-identical");
}

#[test]
fn a_pre_v4_manifest_without_groups_loads_and_stays_lossless() {
    // A v3 manifest (no `groups` key) parses with an empty group list and
    // re-serializes without inventing the key.
    let v3_manifest = CorpusManifest {
        schema_version: 3,
        chunks: vec![minimal_chunk()],
        groups: Vec::new(),
    };
    let v3_json = serde_json::to_string(&v3_manifest).expect("serialize");
    assert!(
        !v3_json.contains("\"groups\""),
        "empty groups must not introduce a key: {v3_json}"
    );

    let back: CorpusManifest = serde_json::from_str(&v3_json).expect("v3 manifest must parse");
    assert!(back.groups.is_empty());
    let json2 = serde_json::to_string(&back).expect("re-serialize");
    assert_eq!(v3_json, json2);
}

// ── schema v3: gesture statistics (melodic-closure note §7.4) ─────────────────

#[test]
fn chunk_meta_with_gesture_roundtrips() {
    let mut meta = minimal_chunk();
    meta.gesture = Some(sample_gesture());

    let json = serde_json::to_string(&meta).expect("serialize");
    assert!(
        json.contains("\"gesture\""),
        "v3 records carry the gesture key"
    );
    let back: ChunkMeta = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.gesture, Some(sample_gesture()));
    let json2 = serde_json::to_string(&back).expect("re-serialize");
    assert_eq!(json, json2, "JSON round-trip must be byte-identical");
}

#[test]
fn chunk_meta_without_gesture_serializes_without_the_key() {
    // A record with no measured gesture stats stays byte-identical to a
    // v1/v2 record: the key is skipped, not written as null.
    let json = serde_json::to_string(&minimal_chunk()).expect("serialize");
    assert!(
        !json.contains("\"gesture\""),
        "absent gesture stats must not introduce a key: {json}"
    );
}

#[test]
fn v2_json_with_structure_but_no_gesture_loads_as_none() {
    // A v2 record (structure measured, gesture predating the bump) parses,
    // reads gesture as None, and re-serializes without inventing the key.
    let mut meta = minimal_chunk();
    meta.structure = Some(sample_metrics());
    meta.gesture = None;
    let v2_json = serde_json::to_string(&meta).expect("serialize");

    let back: ChunkMeta = serde_json::from_str(&v2_json).expect("v2 record must parse");
    assert!(back.gesture.is_none());
    assert_eq!(back.structure, Some(sample_metrics()));
    let json2 = serde_json::to_string(&back).expect("re-serialize");
    assert_eq!(v2_json, json2);
}

// ── schema v2: structure metrics (S14 Phase 3) ────────────────────────────────

#[test]
fn chunk_meta_with_structure_roundtrips() {
    let mut meta = minimal_chunk();
    meta.structure = Some(sample_metrics());

    let json = serde_json::to_string(&meta).expect("serialize");
    assert!(
        json.contains("\"structure\""),
        "v2 records carry the structure key"
    );
    let back: ChunkMeta = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(back.structure, Some(sample_metrics()));
    let json2 = serde_json::to_string(&back).expect("re-serialize");
    assert_eq!(json, json2, "JSON round-trip must be byte-identical");
}

#[test]
fn chunk_meta_without_structure_serializes_without_the_key() {
    // A record with no measured structure stays byte-identical to a v1
    // record: the key is skipped, not written as null.
    let json = serde_json::to_string(&minimal_chunk()).expect("serialize");
    assert!(
        !json.contains("\"structure\""),
        "absent metrics must not introduce a key: {json}"
    );
}

#[test]
fn v1_json_without_structure_field_loads_as_none() {
    // A v1 record predating the schema bump parses, reads as None, and
    // re-serializes without inventing the key (lossless v1 round-trip).
    let mut meta = minimal_chunk();
    meta.structure = None;
    let v1_json = serde_json::to_string(&meta).expect("serialize");

    let back: ChunkMeta = serde_json::from_str(&v1_json).expect("v1 record must parse");
    assert!(back.structure.is_none());
    let json2 = serde_json::to_string(&back).expect("re-serialize");
    assert_eq!(v1_json, json2);
}

// ── unit tests ─────────────────────────────────────────────────────────────────

#[test]
fn chunk_meta_json_roundtrip() {
    let meta = minimal_chunk();
    let json = serde_json::to_string(&meta).expect("serialize");
    let back: ChunkMeta = serde_json::from_str(&json).expect("deserialize");
    let json2 = serde_json::to_string(&back).expect("re-serialize");
    assert_eq!(json, json2, "JSON round-trip must be byte-identical");
}

#[test]
fn corpus_manifest_json_roundtrip() {
    let manifest = CorpusManifest {
        schema_version: 1,
        chunks: vec![minimal_chunk()],
        groups: Vec::new(),
    };
    let json = serde_json::to_string(&manifest).expect("serialize");
    let back: CorpusManifest = serde_json::from_str(&json).expect("deserialize");
    let json2 = serde_json::to_string(&back).expect("re-serialize");
    assert_eq!(json, json2, "manifest round-trip must be byte-identical");
}

#[test]
fn empty_manifest_roundtrip() {
    let manifest = CorpusManifest {
        schema_version: 1,
        chunks: Vec::new(),
        groups: Vec::new(),
    };
    let json = serde_json::to_string(&manifest).expect("serialize");
    let back: CorpusManifest = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(manifest.schema_version, back.schema_version);
    assert!(back.chunks.is_empty());
}

#[test]
fn all_source_formats_roundtrip() {
    for fmt in [
        SourceFormat::Midi,
        SourceFormat::Gp3,
        SourceFormat::Gp4,
        SourceFormat::Gp5,
        SourceFormat::Gpx,
    ] {
        let json = serde_json::to_string(&fmt).expect("serialize");
        let back: SourceFormat = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(fmt, back);
    }
}

#[test]
fn all_swancore_tags_roundtrip() {
    let tags = vec![
        SwancoreTag::CleanRiff,
        SwancoreTag::SyncopatedRiff,
        SwancoreTag::TappingPassage,
        SwancoreTag::LegatoPassage,
        SwancoreTag::Maj7,
        SwancoreTag::Min7,
        SwancoreTag::Sus2,
        SwancoreTag::Add9,
        SwancoreTag::SlashChord,
        SwancoreTag::PowerChord,
        SwancoreTag::HammerOn,
        SwancoreTag::PullOff,
        SwancoreTag::Slide,
        SwancoreTag::Bend,
        SwancoreTag::Vibrato,
        SwancoreTag::PalmMute,
        SwancoreTag::NaturalHarmonic,
        SwancoreTag::ArtificialHarmonic,
        SwancoreTag::Syncopated,
        SwancoreTag::TripletFeel,
        SwancoreTag::Polyrhythm,
        SwancoreTag::Intro,
        SwancoreTag::Verse,
        SwancoreTag::Chorus,
        SwancoreTag::Bridge,
        SwancoreTag::Outro,
        SwancoreTag::Interlude,
    ];
    let json = serde_json::to_string(&tags).expect("serialize");
    let back: Vec<SwancoreTag> = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(tags, back);
}

#[test]
fn reviewer_decisions_roundtrip() {
    for d in [
        ReviewerDecision::Accepted,
        ReviewerDecision::Rejected,
        ReviewerDecision::NeedsReview,
    ] {
        let json = serde_json::to_string(&d).expect("serialize");
        let back: ReviewerDecision = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(d, back);
    }
}

#[test]
fn fixture_minimal_chunk_loads() {
    let json = include_str!("fixtures/minimal_chunk.json");
    let meta: ChunkMeta = serde_json::from_str(json).expect("fixture must parse");
    assert_eq!(meta.id.0, "fixture_001");
    assert_eq!(meta.tags.len(), 2);
    assert_eq!(meta.quality_flags, vec![QualityFlag::Clean]);
    assert_eq!(meta.reviewer, Some(ReviewerDecision::Accepted));
    assert!(
        meta.structure.is_none(),
        "the v1 fixture predates structure metrics"
    );
}

// ── property tests ─────────────────────────────────────────────────────────────

fn arb_swancore_tag() -> impl Strategy<Value = SwancoreTag> {
    prop_oneof![
        Just(SwancoreTag::CleanRiff),
        Just(SwancoreTag::SyncopatedRiff),
        Just(SwancoreTag::TappingPassage),
        Just(SwancoreTag::LegatoPassage),
        Just(SwancoreTag::Maj7),
        Just(SwancoreTag::PowerChord),
        Just(SwancoreTag::HammerOn),
        Just(SwancoreTag::PullOff),
        Just(SwancoreTag::Slide),
        Just(SwancoreTag::PalmMute),
    ]
}

fn arb_quality_flag() -> impl Strategy<Value = QualityFlag> {
    prop_oneof![
        Just(QualityFlag::Clean),
        Just(QualityFlag::Lossy),
        Just(QualityFlag::Quantized),
        Just(QualityFlag::FlatDynamics),
    ]
}

fn arb_source_format() -> impl Strategy<Value = SourceFormat> {
    prop_oneof![
        Just(SourceFormat::Midi),
        Just(SourceFormat::Gp3),
        Just(SourceFormat::Gp4),
        Just(SourceFormat::Gp5),
        Just(SourceFormat::Gpx),
    ]
}

fn arb_reviewer() -> impl Strategy<Value = Option<ReviewerDecision>> {
    prop_oneof![
        Just(None),
        Just(Some(ReviewerDecision::Accepted)),
        Just(Some(ReviewerDecision::Rejected)),
        Just(Some(ReviewerDecision::NeedsReview)),
    ]
}

/// Sixteenth-step f64 values are exactly representable, so JSON round-trips
/// stay byte-identical (the same trick as the integer-BPM strategy below).
fn arb_structure() -> impl Strategy<Value = Option<StructureMetrics>> {
    let metrics = (
        1_usize..=8,
        prop_opt(1_usize..=4),
        prop_opt(1_u32..=8),
        0_u32..=16,
        0_u32..=16,
        0_u32..=16,
    )
        .prop_map(
            |(bar_count, period_bars, subbar_beats, rep16, loop16, cx16)| StructureMetrics {
                bar_count,
                detected_pattern_period_bars: period_bars,
                detected_pattern_period_ticks: period_bars
                    .map(|p| u32::try_from(p).unwrap_or(1).saturating_mul(1920)),
                detected_subbar_period_ticks: subbar_beats.map(|b| b.saturating_mul(480)),
                repeatability_score: f64::from(rep16) / 16.0,
                variation_score: 1.0 - f64::from(rep16) / 16.0,
                loopability_score: f64::from(loop16) / 16.0,
                structural_complexity: f64::from(cx16) / 16.0,
            },
        );
    prop_opt(metrics)
}

/// Sixteenth-step f64 values are exactly representable, so JSON round-trips
/// stay byte-identical (the same trick as [`arb_structure`]).
fn arb_gesture() -> impl Strategy<Value = Option<GestureStats>> {
    let stats = (
        1_usize..=32,
        1_usize..=8,
        0_usize..=4,
        (0_u32..=64, 0_u32..=64, 0_u32..=16, 0_u32..=16, 0_u32..=16),
    )
        .prop_map(
            |(note_count, burst_count, rest_count, (mean16, rest16, grid16, land16, len16))| {
                GestureStats {
                    note_count,
                    burst_count,
                    mean_burst_notes: f64::from(mean16) / 16.0,
                    max_burst_notes: note_count,
                    rest_count,
                    mean_rest_quarters: f64::from(rest16) / 16.0,
                    rest_on_grid_share: f64::from(grid16) / 16.0,
                    modal_landing_share: f64::from(land16) / 16.0,
                    mean_final_lengthening: f64::from(len16) / 16.0,
                }
            },
        );
    prop_opt(stats)
}

/// Sixteenth-step f64 values are exactly representable, so JSON round-trips
/// stay byte-identical (the same trick as [`arb_structure`]).
fn arb_complexity() -> impl Strategy<Value = Option<ComplexityProfile>> {
    let profile = (
        0_u32..=16,
        0_u32..=16,
        0_u32..=16,
        0_u32..=16,
        0_u32..=16,
        0_u32..=16,
    )
        .prop_map(
            |(rhy16, pit16, tec16, har16, ply16, str16)| ComplexityProfile {
                rhythmic: f64::from(rhy16) / 16.0,
                pitch: f64::from(pit16) / 16.0,
                technical: f64::from(tec16) / 16.0,
                harmonic: f64::from(har16) / 16.0,
                playability: f64::from(ply16) / 16.0,
                structural: f64::from(str16) / 16.0,
            },
        );
    prop_opt(profile)
}

fn arb_cohort() -> impl Strategy<Value = Option<StyleCohort>> {
    prop_oneof![
        Just(None),
        Just(Some(StyleCohort::Core)),
        Just(Some(StyleCohort::Adjacent)),
    ]
}

fn arb_ensemble() -> impl Strategy<Value = Option<EnsembleRef>> {
    let link = ("[a-z][a-z0-9_]{2,12}", 0_u32..=3).prop_map(|(group_id, part_index)| EnsembleRef {
        group_id,
        part_index,
    });
    prop_opt(link)
}

fn arb_rights() -> impl Strategy<Value = Option<RightsInfo>> {
    let status = prop_oneof![
        Just(RightsStatus::PublicDomain),
        Just(RightsStatus::CcBy),
        Just(RightsStatus::CcBySa),
        Just(RightsStatus::CopyrightedComposition),
        Just(RightsStatus::Unknown),
    ];
    let acquisition = prop_oneof![
        Just(Acquisition::CommunityTabSite),
        Just(Acquisition::PurchasedOfficial),
        Just(Acquisition::SelfTranscribed),
        Just(Acquisition::OmrFromScan),
        Just(Acquisition::ArtistProvided),
    ];
    // ASCII notes (no quotes/backslashes) keep the JSON round-trip byte-identical.
    let info = (
        status,
        acquisition,
        any::<bool>(),
        "[A-Za-z0-9 :/._-]{0,40}",
    )
        .prop_map(
            |(rights_status, acquisition, redistributable, notes)| RightsInfo {
                rights_status,
                acquisition,
                redistributable,
                notes,
            },
        );
    prop_opt(info)
}

/// Strategy: an optional near-duplicate link with a dyadic quote share, so the
/// JSON round-trip stays byte-identical (#76, schema v8).
fn arb_duplicate() -> impl Strategy<Value = Option<PhraseDuplicate>> {
    prop_opt(
        (
            0_usize..16,
            prop_oneof![
                Just(0.0_f64),
                Just(0.5),
                Just(0.75),
                Just(0.9375),
                Just(1.0)
            ],
        )
            .prop_map(|(of, quote_share)| PhraseDuplicate { of, quote_share }),
    )
}

proptest! {
    #[test]
    fn prop_chunk_meta_json_roundtrip(
        id in "[a-z][a-z0-9_]{3,15}",
        title in "[A-Za-z0-9 ]{1,50}",
        filename in "[a-z][a-z0-9_]{2,12}",
        fmt in arb_source_format(),
        // Use integer BPM values to avoid f64 shortest-representation edge cases
        // in serde_json: integer-valued f64 always roundtrip through JSON exactly.
        tempo_bpm_int in 40u32..=300u32,
        tpq in prop_oneof![Just(480u16), Just(960u16)],
        numerator in 2u8..=12u8,
        tags in prop_vec(arb_swancore_tag(), 0..4),
        flags in prop_vec(arb_quality_flag(), 0..3),
        reviewer in arb_reviewer(),
        structure in arb_structure(),
        gesture in arb_gesture(),
        complexity in arb_complexity(),
        style_cohort in arb_cohort(),
        ensemble in arb_ensemble(),
        rights in arb_rights(),
        duplicate in arb_duplicate(),
    ) {
        let meta = ChunkMeta {
            id: ChunkId(id),
            title,
            source: SourceRef {
                filename,
                format: fmt,
                bar_range: None,
            },
            tempo_bpm: f64::from(tempo_bpm_int),
            ticks_per_quarter: tpq,
            time_signature: (numerator, 4),
            tuning: "standard_e".to_owned(),
            tags,
            boundaries: Vec::new(),
            techniques: Vec::new(),
            quality_flags: flags,
            reviewer,
            structure,
            gesture,
            complexity,
            duplicate,
            style_cohort,
            ensemble,
            rights,
            created_at: "2026-05-20T00:00:00Z".to_owned(),
            updated_at: "2026-05-20T00:00:00Z".to_owned(),
        };
        let json = serde_json::to_string(&meta).expect("serialize");
        let back: ChunkMeta = serde_json::from_str(&json).expect("deserialize");
        let json2 = serde_json::to_string(&back).expect("re-serialize");
        prop_assert_eq!(json, json2);
    }
}
