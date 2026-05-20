// TDD red phase for S8 curation layer.
// Fails to compile until `griff_preview::curation` is implemented.
#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::missing_assert_message,
    clippy::absolute_paths,
    clippy::missing_const_for_fn
)]

use griff_core::corpus::{
    BoundaryEntry, ChunkId, ChunkMeta, QualityFlag, ReviewerDecision, SourceFormat, SourceRef,
    SwancoreTag,
};
use std::path::Path;
use griff_preview::curation::{apply_curation, load_chunk_meta, save_chunk_meta, CurationAction};

// ── helpers ───────────────────────────────────────────────────────────────────

fn sample_chunk() -> ChunkMeta {
    ChunkMeta {
        id: ChunkId("test-chunk-001".to_owned()),
        title: "Test Chunk".to_owned(),
        source: SourceRef {
            filename: "test.mid".to_owned(),
            format: SourceFormat::Midi,
            bar_range: None,
        },
        tempo_bpm: 120.0,
        ticks_per_quarter: 480,
        time_signature: (4, 4),
        tuning: "standard_e".to_owned(),
        tags: Vec::new(),
        boundaries: Vec::new(),
        techniques: Vec::new(),
        quality_flags: Vec::new(),
        reviewer: None,
        created_at: "2026-01-01T00:00:00Z".to_owned(),
        updated_at: "2026-01-01T00:00:00Z".to_owned(),
    }
}

// ── reviewer decision ─────────────────────────────────────────────────────────

#[test]
fn curate_approve_sets_reviewer_accepted() {
    let mut chunk = sample_chunk();
    apply_curation(&mut chunk, CurationAction::Approve);
    assert_eq!(
        chunk.reviewer,
        Some(ReviewerDecision::Accepted),
        "Approve must set reviewer to Accepted"
    );
}

#[test]
fn curate_reject_sets_reviewer_rejected() {
    let mut chunk = sample_chunk();
    apply_curation(&mut chunk, CurationAction::Reject);
    assert_eq!(
        chunk.reviewer,
        Some(ReviewerDecision::Rejected),
        "Reject must set reviewer to Rejected"
    );
}

#[test]
fn curate_approve_overwrites_previous_decision() {
    let mut chunk = sample_chunk();
    chunk.reviewer = Some(ReviewerDecision::Rejected);
    apply_curation(&mut chunk, CurationAction::Approve);
    assert_eq!(
        chunk.reviewer,
        Some(ReviewerDecision::Accepted),
        "Approve must overwrite a prior Rejected decision"
    );
}

// ── tag management ────────────────────────────────────────────────────────────

#[test]
fn curate_add_tag_appends_to_tags() {
    let mut chunk = sample_chunk();
    apply_curation(
        &mut chunk,
        CurationAction::AddTag(SwancoreTag::Polyrhythm),
    );
    assert!(
        chunk.tags.contains(&SwancoreTag::Polyrhythm),
        "AddTag must append the tag to chunk.tags"
    );
}

#[test]
fn curate_add_tag_is_idempotent() {
    let mut chunk = sample_chunk();
    apply_curation(
        &mut chunk,
        CurationAction::AddTag(SwancoreTag::Polyrhythm),
    );
    apply_curation(
        &mut chunk,
        CurationAction::AddTag(SwancoreTag::Polyrhythm),
    );
    let count = chunk
        .tags
        .iter()
        .filter(|&&t| t == SwancoreTag::Polyrhythm)
        .count();
    assert_eq!(count, 1, "AddTag must not duplicate an already-present tag");
}

#[test]
fn curate_remove_tag_removes_tag() {
    let mut chunk = sample_chunk();
    chunk.tags.push(SwancoreTag::Polyrhythm);
    apply_curation(
        &mut chunk,
        CurationAction::RemoveTag(SwancoreTag::Polyrhythm),
    );
    assert!(
        !chunk.tags.contains(&SwancoreTag::Polyrhythm),
        "RemoveTag must remove the tag from chunk.tags"
    );
}

#[test]
fn curate_remove_absent_tag_is_noop() {
    let mut chunk = sample_chunk();
    let before = chunk.tags.clone();
    apply_curation(
        &mut chunk,
        CurationAction::RemoveTag(SwancoreTag::Polyrhythm),
    );
    assert_eq!(
        chunk.tags, before,
        "RemoveTag on absent tag must leave tags unchanged"
    );
}

// ── quality flags ─────────────────────────────────────────────────────────────

#[test]
fn curate_add_quality_flag_appends() {
    let mut chunk = sample_chunk();
    apply_curation(&mut chunk, CurationAction::AddQualityFlag(QualityFlag::Lossy));
    assert!(
        chunk.quality_flags.contains(&QualityFlag::Lossy),
        "AddQualityFlag must append the flag"
    );
}

#[test]
fn curate_add_quality_flag_is_idempotent() {
    let mut chunk = sample_chunk();
    apply_curation(&mut chunk, CurationAction::AddQualityFlag(QualityFlag::Lossy));
    apply_curation(&mut chunk, CurationAction::AddQualityFlag(QualityFlag::Lossy));
    let count = chunk
        .quality_flags
        .iter()
        .filter(|&&f| f == QualityFlag::Lossy)
        .count();
    assert_eq!(count, 1, "AddQualityFlag must not duplicate an existing flag");
}

// ── set title ─────────────────────────────────────────────────────────────────

#[test]
fn curate_set_title_updates_title() {
    let mut chunk = sample_chunk();
    apply_curation(
        &mut chunk,
        CurationAction::SetTitle("New Title".to_owned()),
    );
    assert_eq!(
        chunk.title, "New Title",
        "SetTitle must update chunk.title"
    );
}

// ── persistence ───────────────────────────────────────────────────────────────

#[test]
fn save_load_roundtrip() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let path = dir.path().join("chunk.json");
    let original = sample_chunk();
    save_chunk_meta(&path, &original).expect("save must succeed");
    let loaded = load_chunk_meta(&path).expect("load must succeed");
    assert_eq!(
        loaded, original,
        "load after save must produce equal ChunkMeta"
    );
}

#[test]
fn load_nonexistent_path_returns_error() {
    let result = load_chunk_meta(Path::new("/tmp/griff_nonexistent_chunk.json"));
    assert!(
        result.is_err(),
        "loading a nonexistent file must return Err"
    );
}

#[test]
fn save_load_with_all_fields_populated() {
    let dir = tempfile::tempdir().expect("create temp dir");
    let path = dir.path().join("full_chunk.json");
    let mut chunk = sample_chunk();
    chunk.tags.push(SwancoreTag::Polyrhythm);
    chunk.quality_flags.push(QualityFlag::Quantized);
    chunk.boundaries.push(BoundaryEntry {
        start_tick: 0,
        end_tick: 1920,
        score: 0.9,
    });
    chunk.reviewer = Some(ReviewerDecision::Accepted);
    save_chunk_meta(&path, &chunk).expect("save full chunk");
    let loaded = load_chunk_meta(&path).expect("load full chunk");
    assert_eq!(loaded, chunk, "full-field roundtrip must be lossless");
}
