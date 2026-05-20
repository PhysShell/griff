// TDD red phase for S5: corpus schema round-trip tests.
// These fail to compile until `core/src/corpus.rs` is implemented.

use griff_core::corpus::{
    BoundaryEntry, ChunkId, ChunkMeta, CorpusManifest, QualityFlag, ReviewerDecision,
    SourceFormat, SourceRef, SwancoreTag,
};
use proptest::prelude::*;

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
        created_at: "2026-05-20T00:00:00Z".to_owned(),
        updated_at: "2026-05-20T00:00:00Z".to_owned(),
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
        tempo in 40.0f64..=300.0f64,
        tpq in prop_oneof![Just(480u16), Just(960u16)],
        numerator in 2u8..=12u8,
        tags in proptest::collection::vec(arb_swancore_tag(), 0..4),
        flags in proptest::collection::vec(arb_quality_flag(), 0..3),
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
            tempo_bpm: tempo,
            ticks_per_quarter: tpq,
            time_signature: (numerator, 4),
            tuning: "standard_e".to_owned(),
            tags,
            boundaries: Vec::new(),
            techniques: Vec::new(),
            quality_flags: flags,
            reviewer,
            created_at: "2026-05-20T00:00:00Z".to_owned(),
            updated_at: "2026-05-20T00:00:00Z".to_owned(),
        };
        let json = serde_json::to_string(&meta).expect("serialize");
        let back: ChunkMeta = serde_json::from_str(&json).expect("deserialize");
        let json2 = serde_json::to_string(&back).expect("re-serialize");
        prop_assert_eq!(json, json2);
    }
}
