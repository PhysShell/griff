//! Red → green tests for the curation persistence seam (S8): a viewport
//! decision lands in the chunk record's `reviewer` field, everything else
//! byte-identical. References `griff_preview::curation`, which does not
//! exist yet, so the suite fails to compile until the green step.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::missing_assert_message,
    clippy::str_to_string,
    clippy::indexing_slicing,
    clippy::tuple_array_conversions
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

// ── rename seam (S8 curation slice 4) ───────────────────────────────────────
// TDD red phase: the title writer. References `rename_record` and
// `CurationError::EmptyTitle`, which do not exist yet, so the suite fails
// to compile until the green step.

#[test]
fn rename_record_rewrites_only_the_title() {
    use griff_preview::curation::rename_record;

    let json = serde_json::to_string(&record()).expect("serialize");
    let updated = rename_record(&json, "Verse riff, take 2").expect("update ok");
    let back: ChunkMeta = serde_json::from_str(&updated).expect("updated record parses");

    let mut expected = record();
    expected.title = "Verse riff, take 2".to_owned();
    assert_eq!(back, expected, "only the title field changes");
}

#[test]
fn rename_record_trims_and_rejects_an_empty_title() {
    use griff_preview::curation::rename_record;

    let json = serde_json::to_string(&record()).expect("serialize");
    let updated = rename_record(&json, "  padded  ").expect("update ok");
    let back: ChunkMeta = serde_json::from_str(&updated).expect("parses");
    assert_eq!(back.title, "padded", "surrounding whitespace is trimmed");

    assert_eq!(
        rename_record(&json, "   ").unwrap_err(),
        CurationError::EmptyTitle,
        "a blank title cannot erase the record's name"
    );
}

#[test]
fn rename_record_on_garbage_input_fails() {
    use griff_preview::curation::rename_record;

    assert_eq!(
        rename_record("not json", "x").unwrap_err(),
        CurationError::ParseFailed
    );
}

// ── split/merge seam (S8 curation slice 5) ──────────────────────────────────
// TDD red phase: the record splitter and the record merger. A record's extent
// is its `source.bar_range`; split partitions it at a bar, merge joins two
// same-source adjacent records. Both invalidate what was measured or reviewed
// over the old extent (reviewer + structure/gesture/complexity reset) — the
// halves and the join are new records the curator reviews afresh. References
// `split_record`, `merge_records`, and four `CurationError` variants that do
// not exist yet, so the suite fails to compile until the green step.

fn record_with_range(first_bar: u32, last_bar: u32) -> ChunkMeta {
    let mut meta = record();
    meta.source.bar_range = Some((first_bar, last_bar));
    meta
}

/// One source bar at 960 TPQ in 4/4.
const BAR: u32 = 3840;

#[test]
fn split_record_partitions_the_bar_range() {
    use griff_preview::curation::split_record;

    let json = serde_json::to_string(&record_with_range(0, 4)).expect("serialize");
    let (a, b) = split_record(&json, 2).expect("split ok");
    let first: ChunkMeta = serde_json::from_str(&a).expect("first half parses");
    let second: ChunkMeta = serde_json::from_str(&b).expect("second half parses");

    assert_eq!(first.source.bar_range, Some((0, 1)));
    assert_eq!(second.source.bar_range, Some((2, 4)));
    assert_eq!(first.id.0, "cur_001.1", "halves get derived ids");
    assert_eq!(second.id.0, "cur_001.2");
    assert_eq!(first.title, "Curated (1/2)", "halves get derived titles");
    assert_eq!(second.title, "Curated (2/2)");
    for half in [&first, &second] {
        assert_eq!(half.source.filename, "cur.mid", "source carries over");
        assert_eq!(half.tags, vec![SwancoreTag::CleanRiff], "tags carry over");
        assert_eq!(half.ticks_per_quarter, 960);
        assert_eq!(half.tuning, "standard_e");
    }
}

#[test]
fn split_record_partitions_boundaries_at_the_split_tick() {
    use griff_core::corpus::BoundaryEntry;
    use griff_preview::curation::split_record;

    let mut meta = record_with_range(0, 4);
    meta.boundaries = vec![
        BoundaryEntry {
            start_tick: 0,
            end_tick: BAR,
            score: 0.9,
        },
        // Straddles the split point: stays in the first half, clamped.
        BoundaryEntry {
            start_tick: BAR,
            end_tick: 3 * BAR,
            score: 0.8,
        },
        BoundaryEntry {
            start_tick: 3 * BAR,
            end_tick: 4 * BAR,
            score: 0.7,
        },
    ];
    let json = serde_json::to_string(&meta).expect("serialize");
    let (a, b) = split_record(&json, 2).expect("split ok");
    let first: ChunkMeta = serde_json::from_str(&a).expect("parses");
    let second: ChunkMeta = serde_json::from_str(&b).expect("parses");

    assert_eq!(first.boundaries.len(), 2);
    assert_eq!(
        (first.boundaries[1].start_tick, first.boundaries[1].end_tick),
        (BAR, 2 * BAR),
        "a straddling boundary is clamped to the split tick"
    );
    assert_eq!(second.boundaries.len(), 1);
    assert_eq!(
        (
            second.boundaries[0].start_tick,
            second.boundaries[0].end_tick
        ),
        (BAR, 2 * BAR),
        "second-half boundaries are rebased to its start"
    );
}

#[test]
fn split_record_resets_review_and_measured_metrics() {
    use griff_core::structure::ComplexityProfile;
    use griff_preview::curation::split_record;

    let mut meta = record_with_range(0, 4);
    meta.reviewer = Some(ReviewerDecision::Accepted);
    meta.complexity = Some(ComplexityProfile {
        rhythmic: 0.5,
        pitch: 0.5,
        technical: 0.5,
        harmonic: 0.5,
        playability: 0.5,
        structural: 0.5,
    });
    let json = serde_json::to_string(&meta).expect("serialize");
    let (a, b) = split_record(&json, 2).expect("split ok");
    for half in [a, b] {
        let half: ChunkMeta = serde_json::from_str(&half).expect("parses");
        assert_eq!(half.reviewer, None, "a half is a new record to review");
        assert_eq!(half.complexity, None, "whole-extent measurements drop");
    }
}

#[test]
fn split_record_rejects_an_out_of_range_point() {
    use griff_preview::curation::split_record;

    let json = serde_json::to_string(&record_with_range(2, 6)).expect("serialize");
    for at_bar in [0, 1, 2, 7] {
        assert_eq!(
            split_record(&json, at_bar).unwrap_err(),
            CurationError::SplitOutOfRange,
            "the second half must start strictly inside the range"
        );
    }
    assert!(
        split_record(&json, 3).is_ok(),
        "the first interior bar works"
    );
    assert!(split_record(&json, 6).is_ok(), "the last bar works");
}

#[test]
fn split_record_requires_a_bar_range() {
    use griff_preview::curation::split_record;

    let mut meta = record();
    meta.source.bar_range = None;
    let json = serde_json::to_string(&meta).expect("serialize");
    assert_eq!(
        split_record(&json, 1).unwrap_err(),
        CurationError::MissingBarRange
    );
}

#[test]
fn split_record_on_garbage_input_fails() {
    use griff_preview::curation::split_record;

    assert_eq!(
        split_record("not json", 1).unwrap_err(),
        CurationError::ParseFailed
    );
}

#[test]
fn merge_records_joins_adjacent_same_source_records() {
    use griff_core::corpus::BoundaryEntry;
    use griff_preview::curation::merge_records;

    let mut first = record_with_range(0, 1);
    first.boundaries = vec![BoundaryEntry {
        start_tick: 0,
        end_tick: BAR,
        score: 0.9,
    }];
    let mut second = record_with_range(2, 4);
    second.id = ChunkId("cur_002".to_owned());
    second.title = "Other".to_owned();
    second.tags = vec![SwancoreTag::CleanRiff, SwancoreTag::Maj7];
    second.boundaries = vec![BoundaryEntry {
        start_tick: 0,
        end_tick: BAR,
        score: 0.8,
    }];
    let a = serde_json::to_string(&first).expect("serialize");
    let b = serde_json::to_string(&second).expect("serialize");

    let merged = merge_records(&a, &b).expect("merge ok");
    let back: ChunkMeta = serde_json::from_str(&merged).expect("parses");
    assert_eq!(back.source.bar_range, Some((0, 4)));
    assert_eq!(back.id.0, "cur_001", "the first record's identity wins");
    assert_eq!(back.title, "Curated");
    assert_eq!(
        back.tags,
        vec![SwancoreTag::CleanRiff, SwancoreTag::Maj7],
        "tags are an order-preserving union"
    );
    assert_eq!(back.boundaries.len(), 2);
    assert_eq!(
        (back.boundaries[1].start_tick, back.boundaries[1].end_tick),
        (2 * BAR, 3 * BAR),
        "the second record's boundaries are rebased past the first's span"
    );
    assert_eq!(back.reviewer, None, "the join is a new record to review");
}

#[test]
fn merge_records_rejects_non_adjacent_ranges() {
    use griff_preview::curation::merge_records;

    let a = serde_json::to_string(&record_with_range(0, 1)).expect("serialize");
    let gap = serde_json::to_string(&record_with_range(3, 4)).expect("serialize");
    assert_eq!(
        merge_records(&a, &gap).unwrap_err(),
        CurationError::NotAdjacent
    );
    let b = serde_json::to_string(&record_with_range(2, 4)).expect("serialize");
    assert_eq!(
        merge_records(&b, &a).unwrap_err(),
        CurationError::NotAdjacent,
        "order matters: the first record must come first in the source"
    );
}

#[test]
fn merge_records_rejects_a_source_or_timing_mismatch() {
    use griff_preview::curation::merge_records;

    let a = serde_json::to_string(&record_with_range(0, 1)).expect("serialize");

    let mut other_file = record_with_range(2, 4);
    other_file.source.filename = "elsewhere.mid".to_owned();
    let b = serde_json::to_string(&other_file).expect("serialize");
    assert_eq!(
        merge_records(&a, &b).unwrap_err(),
        CurationError::MergeMismatch,
        "records from different sources never merge"
    );

    let mut other_grid = record_with_range(2, 4);
    other_grid.ticks_per_quarter = 480;
    let c = serde_json::to_string(&other_grid).expect("serialize");
    assert_eq!(
        merge_records(&a, &c).unwrap_err(),
        CurationError::MergeMismatch,
        "a tick-grid mismatch cannot be concatenated"
    );
}

#[test]
fn merge_records_requires_both_bar_ranges() {
    use griff_preview::curation::merge_records;

    let a = serde_json::to_string(&record_with_range(0, 1)).expect("serialize");
    let mut unranged = record();
    unranged.source.bar_range = None;
    let b = serde_json::to_string(&unranged).expect("serialize");
    assert_eq!(
        merge_records(&a, &b).unwrap_err(),
        CurationError::MissingBarRange
    );
}

#[test]
fn merge_records_keeps_a_shared_cohort_and_drops_a_disagreement() {
    use griff_core::corpus::StyleCohort;
    use griff_preview::curation::merge_records;

    let mut first = record_with_range(0, 1);
    first.style_cohort = Some(StyleCohort::Core);
    let mut second = record_with_range(2, 4);
    second.style_cohort = Some(StyleCohort::Core);
    let a = serde_json::to_string(&first).expect("serialize");
    let b = serde_json::to_string(&second).expect("serialize");
    let merged: ChunkMeta =
        serde_json::from_str(&merge_records(&a, &b).expect("merge ok")).expect("parses");
    assert_eq!(
        merged.style_cohort,
        Some(StyleCohort::Core),
        "agreement holds"
    );

    second.style_cohort = Some(StyleCohort::Adjacent);
    let disagreeing = serde_json::to_string(&second).expect("serialize");
    let dropped: ChunkMeta =
        serde_json::from_str(&merge_records(&a, &disagreeing).expect("merge ok")).expect("parses");
    assert_eq!(dropped.style_cohort, None, "a disagreement is dropped");
}

#[test]
fn merge_records_on_garbage_input_fails() {
    use griff_preview::curation::merge_records;

    let a = serde_json::to_string(&record_with_range(0, 1)).expect("serialize");
    assert_eq!(
        merge_records("not json", &a).unwrap_err(),
        CurationError::ParseFailed
    );
    assert_eq!(
        merge_records(&a, "not json").unwrap_err(),
        CurationError::ParseFailed
    );
}

// ── split at a tick (S8 curation slice 5 — the shell's playhead mapping) ────
// TDD red phase: the TUI pins a split to a playhead *tick*; the shell maps
// it to the absolute source bar containing that tick (the split lands on
// the bar boundary at or before the playhead). References
// `split_record_at_tick`, which does not exist yet, so the suite fails to
// compile until the green step.

#[test]
fn split_record_at_tick_floors_to_the_containing_bar() {
    use griff_preview::curation::split_record_at_tick;

    let json = serde_json::to_string(&record_with_range(0, 4)).expect("serialize");
    let (a, b) = split_record_at_tick(&json, 2 * BAR + 100).expect("split ok");
    let first: ChunkMeta = serde_json::from_str(&a).expect("parses");
    let second: ChunkMeta = serde_json::from_str(&b).expect("parses");
    assert_eq!(first.source.bar_range, Some((0, 1)));
    assert_eq!(second.source.bar_range, Some((2, 4)));

    // The tick is chunk-relative; the bar range may start past zero.
    let offset = serde_json::to_string(&record_with_range(2, 6)).expect("serialize");
    let (off_a, off_b) = split_record_at_tick(&offset, BAR + 10).expect("split ok");
    let off_first: ChunkMeta = serde_json::from_str(&off_a).expect("parses");
    let off_second: ChunkMeta = serde_json::from_str(&off_b).expect("parses");
    assert_eq!(off_first.source.bar_range, Some((2, 2)));
    assert_eq!(off_second.source.bar_range, Some((3, 6)));
}

#[test]
fn split_record_at_a_tick_inside_the_first_bar_is_out_of_range() {
    use griff_preview::curation::split_record_at_tick;

    let json = serde_json::to_string(&record_with_range(0, 4)).expect("serialize");
    assert_eq!(
        split_record_at_tick(&json, BAR - 1).unwrap_err(),
        CurationError::SplitOutOfRange,
        "flooring into the first bar would leave an empty first half"
    );
}
