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
    SourceRef, StructureSnapshot, SwancoreTag, CORPUS_SCHEMA_VERSION,
};
use griff_core::structure::StructureMetrics;
use proptest::{collection::vec as prop_vec, prelude::*};

// ── helpers ────────────────────────────────────────────────────────────────────

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

fn snapshot() -> StructureSnapshot {
    StructureSnapshot {
        bar_count: 4,
        pattern_period_bars: Some(1),
        pattern_period_ticks: Some(1920),
        repeatability: 1.0,
        variation: 0.0,
        loopability: 0.75,
        structural_complexity: 0.25,
    }
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
}

// ── structure snapshot (schema v2, S14 Phase 3) ────────────────────────────────

#[test]
fn corpus_schema_version_is_bumped_to_2() {
    assert_eq!(
        CORPUS_SCHEMA_VERSION, 2,
        "persisting structure metrics bumps the schema (ADR-0015)"
    );
}

#[test]
fn chunk_meta_with_structure_roundtrips() {
    let mut meta = minimal_chunk();
    meta.structure = Some(snapshot());

    let json = serde_json::to_string(&meta).expect("serialize");
    assert!(
        json.contains("\"pattern_period_bars\""),
        "snapshot fields serialize snake_case: {json}"
    );
    let back: ChunkMeta = serde_json::from_str(&json).expect("deserialize");
    let json2 = serde_json::to_string(&back).expect("re-serialize");
    assert_eq!(json, json2, "JSON round-trip must be byte-identical");
    assert_eq!(back.structure, Some(snapshot()));
}

#[test]
fn chunk_without_structure_omits_the_field_and_v1_records_still_parse() {
    // Serialising a None snapshot omits the field entirely, so a v2 writer
    // emits v1-shaped JSON for unanalysed chunks…
    let json = serde_json::to_string(&minimal_chunk()).expect("serialize");
    assert!(
        !json.contains("\"structure\""),
        "None snapshot must not serialize: {json}"
    );

    // …and a v1 record (no `structure` key) deserialises as None — the
    // existing fixture is exactly such a record.
    let fixture = include_str!("fixtures/minimal_chunk.json");
    let meta: ChunkMeta = serde_json::from_str(fixture).expect("v1 fixture must parse");
    assert_eq!(meta.structure, None);
}

#[test]
fn structure_snapshot_converts_from_measured_metrics() {
    let metrics = StructureMetrics {
        bar_count: 8,
        detected_pattern_period_bars: Some(2),
        detected_pattern_period_ticks: Some(3840),
        repeatability_score: 0.75,
        variation_score: 0.25,
        loopability_score: 0.5,
        structural_complexity: 0.5,
    };
    let snap = StructureSnapshot::from(metrics);
    assert_eq!(snap.bar_count, 8);
    assert_eq!(snap.pattern_period_bars, Some(2));
    assert_eq!(snap.pattern_period_ticks, Some(3840));
    assert!((snap.repeatability - 0.75).abs() < 1e-12);
    assert!((snap.variation - 0.25).abs() < 1e-12);
    assert!((snap.loopability - 0.5).abs() < 1e-12);
    assert!((snap.structural_complexity - 0.5).abs() < 1e-12);
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
            structure: None,
            created_at: "2026-05-20T00:00:00Z".to_owned(),
            updated_at: "2026-05-20T00:00:00Z".to_owned(),
        };
        let json = serde_json::to_string(&meta).expect("serialize");
        let back: ChunkMeta = serde_json::from_str(&json).expect("deserialize");
        let json2 = serde_json::to_string(&back).expect("re-serialize");
        prop_assert_eq!(json, json2);
    }
}
