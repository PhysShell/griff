// TDD red phase for S5: corpus schema round-trip tests.
// These fail to compile until `core/src/corpus.rs` is implemented.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::missing_assert_message
)]

use griff_core::corpus::{
    BoundaryEntry, ChunkId, ChunkMeta, CorpusManifest, QualityFlag, ReviewerDecision, SourceFormat,
    SourceRef, SwancoreTag, SCHEMA_VERSION,
};
use griff_core::structure::StructureMetrics;
use proptest::{collection::vec as prop_vec, option::of as prop_opt, prelude::*};

// ── helpers ────────────────────────────────────────────────────────────────────

/// Exactly-representable f64 values keep JSON round-trips byte-identical.
const fn sample_metrics() -> StructureMetrics {
    StructureMetrics {
        bar_count: 4,
        detected_pattern_period_bars: Some(1),
        detected_pattern_period_ticks: Some(1920),
        repeatability_score: 0.9375,
        variation_score: 0.0625,
        loopability_score: 0.875,
        structural_complexity: 0.25,
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
        created_at: "2026-05-20T00:00:00Z".to_owned(),
        updated_at: "2026-05-20T00:00:00Z".to_owned(),
    }
}

// ── schema v2: structure metrics (S14 Phase 3) ────────────────────────────────

#[test]
fn schema_version_is_2() {
    assert_eq!(SCHEMA_VERSION, 2, "S14 Phase 3 bumps the corpus schema");
}

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
        0_u32..=16,
        0_u32..=16,
        0_u32..=16,
    )
        .prop_map(
            |(bar_count, period_bars, rep16, loop16, cx16)| StructureMetrics {
                bar_count,
                detected_pattern_period_bars: period_bars,
                detected_pattern_period_ticks: period_bars
                    .map(|p| u32::try_from(p).unwrap_or(1).saturating_mul(1920)),
                repeatability_score: f64::from(rep16) / 16.0,
                variation_score: 1.0 - f64::from(rep16) / 16.0,
                loopability_score: f64::from(loop16) / 16.0,
                structural_complexity: f64::from(cx16) / 16.0,
            },
        );
    prop_opt(metrics)
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
            created_at: "2026-05-20T00:00:00Z".to_owned(),
            updated_at: "2026-05-20T00:00:00Z".to_owned(),
        };
        let json = serde_json::to_string(&meta).expect("serialize");
        let back: ChunkMeta = serde_json::from_str(&json).expect("deserialize");
        let json2 = serde_json::to_string(&back).expect("re-serialize");
        prop_assert_eq!(json, json2);
    }
}
