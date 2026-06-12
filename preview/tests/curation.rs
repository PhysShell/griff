//! Red → green tests for the curation persistence seam (S8): a viewport
//! decision lands in the chunk record's `reviewer` field, everything else
//! byte-identical. References `griff_preview::curation`, which does not
//! exist yet, so the suite fails to compile until the green step.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::missing_assert_message,
    clippy::str_to_string
)]

use griff_core::corpus::{
    ChunkId, ChunkMeta, QualityFlag, ReviewerDecision, SourceFormat, SourceRef, SwancoreTag,
};
use griff_preview::curation::{decide_record, CurationError};
use griff_preview::viewport::CurationDecision;

fn record() -> ChunkMeta {
    ChunkMeta {
        id: ChunkId("cur_001".to_owned()),
        title: "Curated".to_owned(),
        source: SourceRef {
            filename: "cur.mid".to_owned(),
            format: SourceFormat::Midi,
            bar_range: Some((0, 4)),
        },
        tempo_bpm: 140.0,
        ticks_per_quarter: 960,
        time_signature: (4, 4),
        tuning: "standard_e".to_owned(),
        tags: vec![SwancoreTag::CleanRiff],
        boundaries: Vec::new(),
        techniques: Vec::new(),
        quality_flags: vec![QualityFlag::Clean],
        reviewer: None,
        structure: None,
        gesture: None,
        complexity: None,
        style_cohort: None,
        ensemble: None,
        created_at: "2026-06-11T00:00:00Z".to_owned(),
        updated_at: "2026-06-11T00:00:00Z".to_owned(),
    }
}

#[test]
fn approve_sets_the_reviewer_and_keeps_the_rest() {
    let json = serde_json::to_string(&record()).expect("serialize");
    let updated = decide_record(&json, CurationDecision::Approve).expect("update ok");
    let back: ChunkMeta = serde_json::from_str(&updated).expect("updated record parses");

    let mut expected = record();
    expected.reviewer = Some(ReviewerDecision::Accepted);
    assert_eq!(back, expected, "only the reviewer field changes");
}

#[test]
fn reject_maps_to_rejected() {
    let json = serde_json::to_string(&record()).expect("serialize");
    let updated = decide_record(&json, CurationDecision::Reject).expect("update ok");
    let back: ChunkMeta = serde_json::from_str(&updated).expect("updated record parses");
    assert_eq!(back.reviewer, Some(ReviewerDecision::Rejected));
}

#[test]
fn overwriting_an_earlier_decision_is_allowed() {
    // Re-curation is the established healing path: a record that was
    // rejected once can be approved later.
    let mut meta = record();
    meta.reviewer = Some(ReviewerDecision::Rejected);
    let json = serde_json::to_string(&meta).expect("serialize");
    let updated = decide_record(&json, CurationDecision::Approve).expect("update ok");
    let back: ChunkMeta = serde_json::from_str(&updated).expect("parses");
    assert_eq!(back.reviewer, Some(ReviewerDecision::Accepted));
}

#[test]
fn malformed_json_is_a_typed_error() {
    assert!(matches!(
        decide_record("not a record", CurationDecision::Approve),
        Err(CurationError::ParseFailed)
    ));
}

// ── record summary (S8 curation slice: surface the record's current state) ──
// TDD red phase: the curator must *see* the loaded record's curation state
// (title, reviewer, tags) before rename/tag can build on it. References
// `summarize_record` and `RecordSummary`, which do not exist yet, so the
// suite fails to compile until the green step.

#[test]
fn summarize_record_surfaces_title_reviewer_and_tags() {
    use griff_preview::curation::summarize_record;

    let mut meta = record();
    meta.reviewer = Some(ReviewerDecision::Accepted);
    meta.tags = vec![SwancoreTag::CleanRiff, SwancoreTag::Maj7];
    let json = serde_json::to_string(&meta).expect("serialize");

    let summary = summarize_record(&json).expect("summary ok");
    assert_eq!(summary.title, "Curated");
    assert_eq!(summary.reviewer.as_deref(), Some("accepted"));
    assert_eq!(
        summary.tags,
        vec!["clean_riff".to_owned(), "maj7".to_owned()],
        "tags use the schema's snake_case wire names"
    );
}

#[test]
fn summarize_record_handles_an_unreviewed_record() {
    use griff_preview::curation::summarize_record;

    let json = serde_json::to_string(&record()).expect("serialize");
    let summary = summarize_record(&json).expect("summary ok");
    assert_eq!(summary.reviewer, None);
}

#[test]
fn garbage_summary_input_fails() {
    use griff_preview::curation::summarize_record;

    assert_eq!(
        summarize_record("not json").unwrap_err(),
        CurationError::ParseFailed
    );
}

// ── tag editing seam (S8 curation slice 3) ──────────────────────────────────
// TDD red phase: the tag palette and the tags writer. References
// `tag_palette`, `set_tags`, and `CurationError::UnknownTag`, which do not
// exist yet, so the suite fails to compile until the green step.

#[test]
fn tag_palette_covers_every_schema_variant() {
    use griff_preview::curation::tag_palette;

    let palette = tag_palette();
    assert_eq!(
        palette.len(),
        SwancoreTag::all_variants().len(),
        "one palette entry per schema variant"
    );
    for (name, variant) in palette.iter().zip(SwancoreTag::all_variants()) {
        let parsed: SwancoreTag = serde_json::from_value(serde_json::Value::String(name.clone()))
            .expect("every palette name parses back");
        assert_eq!(parsed, *variant, "palette order mirrors all_variants");
    }
    assert!(palette.contains(&"clean_riff".to_owned()));
}

#[test]
fn set_tags_rewrites_only_the_tags() {
    use griff_preview::curation::set_tags;

    let json = serde_json::to_string(&record()).expect("serialize");
    let updated =
        set_tags(&json, &["maj7".to_owned(), "power_chord".to_owned()]).expect("update ok");
    let back: ChunkMeta = serde_json::from_str(&updated).expect("updated record parses");

    let mut expected = record();
    expected.tags = vec![SwancoreTag::Maj7, SwancoreTag::PowerChord];
    assert_eq!(back, expected, "only the tags field changes");
}

#[test]
fn set_tags_rejects_an_unknown_name() {
    use griff_preview::curation::set_tags;

    let json = serde_json::to_string(&record()).expect("serialize");
    assert_eq!(
        set_tags(&json, &["not_a_tag".to_owned()]).unwrap_err(),
        CurationError::UnknownTag
    );
}

#[test]
fn set_tags_on_garbage_input_fails() {
    use griff_preview::curation::set_tags;

    assert_eq!(
        set_tags("not json", &[]).unwrap_err(),
        CurationError::ParseFailed
    );
}
