//! SWG-CORE-01, step A: what the three canonical index fields do **today**.
//!
//! `MasterBar.index`, `ImportWarning::TrackNameInvalidUtf8.track_index`, and
//! `ImportWarning::TempoApproximated.bar_index` are about to change from
//! `usize` to `u64`. These tests are written before that change and must pass
//! before it, so that "the migration altered no behaviour" is a checked claim
//! rather than a hopeful one. They are characterization tests: the backlog
//! exempts them from the red phase, and they must not be edited to make the
//! migration pass.
//!
//! The point they exist to pin is the one the width decision turns on. A
//! stored index is **not** a vector position (H4): it is an exact fact of its
//! own, an importer may disagree with the ordinal, and on a 64-bit host it may
//! already exceed `u32::MAX`. Narrowing to `u32` would therefore shrink an
//! inhabited canonical model; `u64` removes the platform dependence without
//! shrinking anything. The `>u32::MAX` witnesses below are what makes that
//! sentence checkable.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::missing_assert_message,
    clippy::indexing_slicing
)]

use griff_core::dump::normalize;
use griff_core::event::{Tempo, Ticks, TimeSignature, Tuning};
use griff_core::score::{ImportWarning, LossReport, MasterBar, RepeatMarker, Score, Track, Voice};
use griff_core::semantic_diff::{exact_semantic_diff, SemanticDiffReport};
use griff_core::slice::TickRange;

// ── fixtures ───────────────────────────────────────────────────────────────

fn range(start: u32, end: u32) -> TickRange {
    TickRange::new(Ticks(start), Ticks(end)).expect("ordered range")
}

/// A bar whose stored index is given explicitly, never derived.
fn bar(index: usize, start: u32, end: u32) -> MasterBar {
    MasterBar {
        index,
        tick_range: range(start, end),
        time_signature: TimeSignature::new(4, 4).expect("4/4"),
        tempo: Tempo::from_bpm_integer(120).expect("120 BPM"),
        repeat: RepeatMarker::default(),
    }
}

fn score(master_bars: Vec<MasterBar>) -> Score {
    Score {
        ticks_per_quarter: 480,
        master_bars,
        tracks: Vec::new(),
        source_meta: None,
        loss: LossReport::new(),
    }
}

/// A score carrying one track, so the normalized projection has bars to walk.
fn score_with_track(master_bars: Vec<MasterBar>) -> Score {
    let mut out = score(master_bars);
    out.tracks.push(Track {
        name: None,
        channel: 0,
        voices: vec![Voice {
            id: 0,
            event_groups: Vec::new(),
        }],
        tuning: Tuning::new(Vec::new()),
    });
    out
}

fn differing_fields(report: &SemanticDiffReport) -> Vec<String> {
    report
        .differences
        .iter()
        .map(|d| d.path.to_string())
        .collect()
}

// ── the stored index is its own fact (H4) ──────────────────────────────────

#[test]
fn a_stored_bar_index_is_not_derived_from_its_vector_position() {
    let s = score(vec![bar(5, 0, 1920), bar(2, 1920, 3840)]);
    assert_eq!(s.master_bars[0].index, 5);
    assert_eq!(s.master_bars[1].index, 2);
}

#[test]
fn the_exact_diff_reports_a_changed_stored_index() {
    let expected = score(vec![bar(0, 0, 1920)]);
    let mut actual = expected.clone();
    actual.master_bars[0].index = 9;

    let report = exact_semantic_diff(&expected, &actual);
    assert_eq!(
        differing_fields(&report),
        vec!["score.master_bars[ordinal=0].index"],
        "a changed stored index is exactly one exact difference"
    );
    assert_eq!(report.differences[0].expected.as_deref(), Some("0"));
    assert_eq!(report.differences[0].actual.as_deref(), Some("9"));
}

#[test]
fn the_normalized_projection_carries_the_stored_index_not_the_ordinal() {
    let projected = normalize(&score_with_track(vec![bar(5, 0, 1920), bar(2, 1920, 3840)]));
    let indices: Vec<usize> = projected.tracks[0].bars.iter().map(|b| b.index).collect();
    assert_eq!(
        indices,
        vec![5, 2],
        "the projection copies the stored index and does not renumber"
    );
}

// ── warning payload indices are payload, not lookups ───────────────────────

#[test]
fn warning_payload_indices_survive_unchanged() {
    let mut s = score(Vec::new());
    s.loss
        .add(ImportWarning::TrackNameInvalidUtf8 { track_index: 7 });
    s.loss.add(ImportWarning::TempoApproximated {
        bar_index: 11,
        nearest_micros: 4_200_000,
    });

    // No track 7 and no bar 11 exist in this score. Nothing normalizes the
    // payload against the tree, and nothing may start to.
    assert!(s.tracks.is_empty());
    assert!(s.master_bars.is_empty());
    assert_eq!(
        s.loss.warnings,
        vec![
            ImportWarning::TrackNameInvalidUtf8 { track_index: 7 },
            ImportWarning::TempoApproximated {
                bar_index: 11,
                nearest_micros: 4_200_000,
            },
        ]
    );
}

#[test]
fn the_exact_diff_reports_changed_warning_payload_indices() {
    let mut expected = score(Vec::new());
    expected
        .loss
        .add(ImportWarning::TrackNameInvalidUtf8 { track_index: 1 });
    expected.loss.add(ImportWarning::TempoApproximated {
        bar_index: 3,
        nearest_micros: 500_000,
    });

    let mut actual = expected.clone();
    actual.loss.warnings[0] = ImportWarning::TrackNameInvalidUtf8 { track_index: 2 };
    actual.loss.warnings[1] = ImportWarning::TempoApproximated {
        bar_index: 4,
        nearest_micros: 500_000,
    };

    let report = exact_semantic_diff(&expected, &actual);
    assert_eq!(
        differing_fields(&report),
        vec![
            "score.loss.warnings[0].track_index",
            "score.loss.warnings[1].bar_index",
        ]
    );
}

// ── the inhabited range today, and why `u32` would shrink it ───────────────

/// A stored index above `u32::MAX` is representable **now**, on a 64-bit host.
///
/// This is the whole argument for `u64` over `u32`, so it is checked rather
/// than asserted in prose. The `cfg` is the defect itself: the same score is
/// unrepresentable on a 32-bit target today, which is what H3 called a false
/// portability claim. SWG-CORE-01 removes the `cfg`'s reason to exist; this
/// test records that the value was already inside the model before it did, so
/// the migration cannot be read as having widened the canonical domain.
#[cfg(target_pointer_width = "64")]
mod above_u32_max {
    use super::{bar, differing_fields, exact_semantic_diff, normalize, score, score_with_track};
    use griff_core::score::ImportWarning;

    /// `u32::MAX + 1`, spelled through arithmetic on `u64` so the constant is
    /// unambiguous, then narrowed to the field's current `usize`.
    fn beyond_u32() -> usize {
        usize::try_from(u64::from(u32::MAX) + 1).expect("64-bit host")
    }

    #[test]
    fn a_stored_bar_index_above_u32_max_is_inhabited() {
        let s = score(vec![bar(beyond_u32(), 0, 1920)]);
        assert_eq!(s.master_bars[0].index, 4_294_967_296);
    }

    #[test]
    fn a_stored_bar_index_above_u32_max_survives_the_exact_diff() {
        let expected = score(vec![bar(beyond_u32(), 0, 1920)]);
        let mut actual = expected.clone();
        actual.master_bars[0].index = beyond_u32() + 1;

        let report = exact_semantic_diff(&expected, &actual);
        assert_eq!(
            differing_fields(&report),
            vec!["score.master_bars[ordinal=0].index"]
        );
        assert_eq!(
            report.differences[0].expected.as_deref(),
            Some("4294967296"),
            "the value is compared whole, not truncated to 32 bits"
        );
        assert_eq!(report.differences[0].actual.as_deref(), Some("4294967297"));
    }

    #[test]
    fn a_stored_bar_index_above_u32_max_survives_the_projection() {
        let projected = normalize(&score_with_track(vec![bar(beyond_u32(), 0, 1920)]));
        assert_eq!(projected.tracks[0].bars[0].index, 4_294_967_296);
    }

    #[test]
    fn warning_payload_indices_above_u32_max_are_inhabited() {
        let warning = ImportWarning::TempoApproximated {
            bar_index: beyond_u32(),
            nearest_micros: 500_000,
        };
        let ImportWarning::TempoApproximated { bar_index, .. } = warning else {
            panic!("constructed variant");
        };
        assert_eq!(bar_index, 4_294_967_296);

        let name_warning = ImportWarning::TrackNameInvalidUtf8 {
            track_index: beyond_u32(),
        };
        let ImportWarning::TrackNameInvalidUtf8 { track_index } = name_warning else {
            panic!("constructed variant");
        };
        assert_eq!(track_index, 4_294_967_296);
    }
}
