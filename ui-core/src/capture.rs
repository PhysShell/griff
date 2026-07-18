//! Chunk capture (ADR-0026): measure a track and assemble a real [`ChunkMeta`].
//!
//! This is the shared seam the egui cockpit (ADR-0027) and the M1 playground
//! both build chunks through, so neither can drift from the corpus schema or
//! from each other. Pure: the output is a function of the score plus the
//! curator-supplied [`CaptureInputs`] (SPEC §6).

use griff_core::boundary::{self, BoundaryConfig};
use griff_core::corpus::{
    Acquisition, BoundaryEntry, ChunkId, ChunkMeta, QualityFlag, ReviewerDecision, RightsInfo,
    RightsStatus, SourceFormat, SourceRef, StyleCohort, SwancoreTag,
};
use griff_core::event::Ticks;
use griff_core::score::Score;
use griff_core::{gesture, harmony, structure, syncopation, technique};

/// The curator-supplied capture inputs — the non-derivable provenance and
/// tagging a chunk needs.
///
/// Mirrors the CLI's `griff curate` prompts; the numeric codes follow the CLI's
/// prompt order (see the mappers below), and `created_at`/`updated_at` are
/// caller-supplied so capture stays deterministic.
#[derive(Debug, Clone)]
pub struct CaptureInputs<'a> {
    /// Chunk id; trimmed.
    pub id: &'a str,
    /// Human title; trimmed.
    pub title: &'a str,
    /// Source filename recorded in provenance; empty → `unknown.mid`.
    pub filename: &'a str,
    /// Tuning slug; empty → `standard_e`.
    pub tuning: &'a str,
    /// Style-cohort code (1 = adjacent, else core).
    pub cohort: u32,
    /// Space/comma-separated [`SwancoreTag`] indices.
    pub tags_idx: &'a str,
    /// Space/comma-separated quality-flag indices; empty → `[Clean]`.
    pub quality_idx: &'a str,
    /// Reviewer code (0 accept, 1 reject, 2 needs-review, else none).
    pub reviewer: i32,
    /// Rights-status code (CLI prompt order).
    pub rights_status: u32,
    /// Acquisition code (CLI prompt order).
    pub acquisition: u32,
    /// Whether the source may be redistributed (the export gate, ADR-0027 §4).
    pub redistributable: bool,
    /// Free-form rights notes; trimmed.
    pub notes: &'a str,
    /// RFC3339 creation timestamp (caller-supplied).
    pub created_at: &'a str,
    /// RFC3339 update timestamp (caller-supplied).
    pub updated_at: &'a str,
}

/// Conservative defaults so `..CaptureInputs::default()` never silently stamps
/// permissive legal metadata: `reviewer` is the "none" sentinel (no decision)
/// and `rights_status` falls through to `CopyrightedComposition` (rights
/// reserved), not `PublicDomain` (#98 review). Real callers — the cockpit form,
/// the CLI prompts — set reviewer and rights explicitly.
impl Default for CaptureInputs<'_> {
    fn default() -> Self {
        Self {
            id: "",
            title: "",
            filename: "",
            tuning: "",
            cohort: 0,
            tags_idx: "",
            quality_idx: "",
            reviewer: -1,
            rights_status: u32::MAX,
            acquisition: 0,
            redistributable: false,
            notes: "",
            created_at: "",
            updated_at: "",
        }
    }
}

/// Maps an imported score's source-format tag to the corpus [`SourceFormat`]
/// (mirrors the CLI's `source_format`); an unknown/absent tag falls back to MIDI.
fn source_format(score: &Score) -> SourceFormat {
    match score.source_meta.as_ref().and_then(|m| m.format.as_deref()) {
        Some("GP3") => SourceFormat::Gp3,
        Some("GP4") => SourceFormat::Gp4,
        Some("GP5") => SourceFormat::Gp5,
        Some("GP6") => SourceFormat::Gpx,
        Some("GP7") => SourceFormat::Gp,
        _ => SourceFormat::Midi,
    }
}

/// Detects S4 phrase boundaries for `track_index`, scaling the detector's tick
/// gaps to the score PPQN.
///
/// Matches `griff curate`/`griff phrases`; the capture UI previews these cuts
/// before building a chunk.
#[must_use]
pub fn detect_boundaries(score: &Score, track_index: usize) -> Vec<BoundaryEntry> {
    let ppqn = u32::from(score.ticks_per_quarter);
    let config = BoundaryConfig {
        min_gap: Ticks(ppqn.saturating_mul(2)),
        quantize_ticks: Ticks(ppqn.checked_div(4).unwrap_or(1).max(1)),
        ..BoundaryConfig::default()
    };
    boundary::detect_phrase_boundaries(score, track_index, &config)
        .into_iter()
        .map(|b| BoundaryEntry {
            start_tick: b.start_tick.0,
            end_tick: b.end_tick.0,
            score: b.score,
        })
        .collect()
}

/// Space/comma-separated indices → variants (mirrors the CLI's `parse_indices`).
fn parse_indices<T: Copy>(input: &str, variants: &[T]) -> Vec<T> {
    input
        .split(|c: char| c.is_whitespace() || c == ',')
        .filter_map(|s| s.parse::<usize>().ok())
        .filter_map(|i| variants.get(i).copied())
        .collect()
}

/// Rights-status code (the CLI's prompt order) → enum; unknown → copyrighted.
const fn rights_status_from(code: u32) -> RightsStatus {
    match code {
        0 => RightsStatus::PublicDomain,
        1 => RightsStatus::CcBy,
        2 => RightsStatus::CcBySa,
        4 => RightsStatus::Unknown,
        _ => RightsStatus::CopyrightedComposition,
    }
}

/// Acquisition code (the CLI's prompt order) → enum; unknown → community tab.
const fn acquisition_from(code: u32) -> Acquisition {
    match code {
        1 => Acquisition::PurchasedOfficial,
        2 => Acquisition::SelfTranscribed,
        3 => Acquisition::OmrFromScan,
        4 => Acquisition::ArtistProvided,
        _ => Acquisition::CommunityTabSite,
    }
}

/// Reviewer code → optional decision (anything outside `0..=2` → none).
const fn reviewer_from(code: i32) -> Option<ReviewerDecision> {
    match code {
        0 => Some(ReviewerDecision::Accepted),
        1 => Some(ReviewerDecision::Rejected),
        2 => Some(ReviewerDecision::NeedsReview),
        _ => None,
    }
}

/// Style-cohort code → enum (1 = adjacent, else core).
const fn cohort_from(code: u32) -> StyleCohort {
    if code == 1 {
        StyleCohort::Adjacent
    } else {
        StyleCohort::Core
    }
}

/// Assembles a schema-v7 [`ChunkMeta`] for `track_index` (single-track, no
/// ensemble), mirroring the CLI's `build_chunk_meta`.
///
/// It folds the curator's tags with techniques/harmony/syncopation the notation
/// already states (ADR-0018), measures structure/gesture/complexity +
/// boundaries, and records rights.
///
/// # Errors
/// Returns a message if `track_index` is out of range for the score.
pub fn build_chunk(
    score: &Score,
    track_index: usize,
    input: &CaptureInputs<'_>,
) -> Result<ChunkMeta, String> {
    if track_index >= score.tracks.len() {
        return Err(format!(
            "track {track_index} out of range (score has {} tracks)",
            score.tracks.len()
        ));
    }
    let (tempo_bpm, time_signature) = score.master_bars.first().map_or((120.0, (4u8, 4u8)), |b| {
        (
            b.tempo.0,
            (b.time_signature.numerator, b.time_signature.denominator),
        )
    });

    let tuning = if input.tuning.trim().is_empty() {
        "standard_e".to_owned()
    } else {
        input.tuning.trim().to_owned()
    };
    let chosen_tags = parse_indices(input.tags_idx, SwancoreTag::all_variants());
    let derived = technique::derive_techniques(score, track_index);
    let derived_harmony = harmony::derive_harmony(score, track_index);
    let derived_syncopated = syncopation::derive_syncopated(score, track_index);
    let technique_tags = technique::merge_tags(&chosen_tags, &derived.tags);
    let with_harmony = technique::merge_tags(&technique_tags, &derived_harmony);
    let tags = technique::merge_tags(&with_harmony, &derived_syncopated);
    let all_flags = [
        QualityFlag::Clean,
        QualityFlag::Lossy,
        QualityFlag::Quantized,
        QualityFlag::FlatDynamics,
    ];
    let quality_flags = if input.quality_idx.trim().is_empty() {
        vec![QualityFlag::Clean]
    } else {
        parse_indices(input.quality_idx, &all_flags)
    };
    let filename = if input.filename.trim().is_empty() {
        "unknown.mid".to_owned()
    } else {
        input.filename.trim().to_owned()
    };

    Ok(ChunkMeta {
        id: ChunkId(input.id.trim().to_owned()),
        title: input.title.trim().to_owned(),
        source: SourceRef {
            filename,
            format: source_format(score),
            bar_range: None,
        },
        tempo_bpm,
        ticks_per_quarter: score.ticks_per_quarter,
        time_signature,
        tuning,
        tags,
        boundaries: detect_boundaries(score, track_index),
        techniques: derived.names,
        quality_flags,
        reviewer: reviewer_from(input.reviewer),
        structure: structure::measure_structure(score, track_index).ok(),
        gesture: gesture::measure_gesture(score, track_index).ok(),
        complexity: structure::measure_complexity(score, track_index).ok(),
        duplicate: None,
        style_cohort: Some(cohort_from(input.cohort)),
        ensemble: None,
        rights: Some(RightsInfo {
            rights_status: rights_status_from(input.rights_status),
            acquisition: acquisition_from(input.acquisition),
            redistributable: input.redistributable,
            notes: input.notes.trim().to_owned(),
        }),
        created_at: input.created_at.to_owned(),
        updated_at: input.updated_at.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::*;
    use griff_core::import::import_score_auto;

    fn two_phrases() -> Score {
        import_score_auto(include_bytes!("../../cli/tests/fixtures/two_phrases.mid"))
            .expect("two_phrases.mid imports")
    }

    #[test]
    fn gp7_format_tag_maps_to_the_gp_source_format() {
        // A `.gp` score captured through the cockpit must record its format as
        // GP7, not fall back to MIDI — parity with the CLI's `source_format`.
        use griff_core::score::SourceMeta;
        let mut score = two_phrases();
        score.source_meta = Some(SourceMeta {
            format: Some("GP7".to_owned()),
        });
        assert_eq!(source_format(&score), SourceFormat::Gp);
    }

    #[test]
    fn build_chunk_assembles_a_serializable_schema_chunk() {
        let score = two_phrases();
        let input = CaptureInputs {
            id: "  two_phrases_001  ",
            title: "Two Phrases",
            filename: "two_phrases.mid",
            tuning: "",
            cohort: 1,
            rights_status: 0, // PublicDomain — explicit now that Default is conservative
            redistributable: true,
            notes: "public domain demo",
            created_at: "2026-01-01T00:00:00Z",
            updated_at: "2026-01-02T00:00:00Z",
            ..CaptureInputs::default()
        };
        let chunk = build_chunk(&score, 0, &input).expect("builds a chunk");

        assert_eq!(chunk.id.0, "two_phrases_001", "the id is trimmed");
        assert_eq!(chunk.title, "Two Phrases");
        assert_eq!(chunk.tuning, "standard_e", "an empty tuning defaults");
        assert_eq!(
            chunk.style_cohort,
            Some(StyleCohort::Adjacent),
            "cohort 1 = adjacent"
        );
        let rights = chunk.rights.as_ref().expect("rights captured");
        assert_eq!(rights.rights_status, RightsStatus::PublicDomain);
        assert!(rights.redistributable);
        assert_eq!(chunk.created_at, "2026-01-01T00:00:00Z");

        // Byte-compatible with what `griff manifest` reads.
        let json = serde_json::to_string(&chunk).expect("serializes");
        assert!(
            json.contains("two_phrases_001"),
            "the chunk serializes its id"
        );
    }

    #[test]
    fn build_chunk_rejects_an_out_of_range_track() {
        let score = two_phrases();
        let err = build_chunk(&score, 99, &CaptureInputs::default()).expect_err("out of range");
        assert!(err.contains("out of range"), "names the problem: {err}");
    }

    #[test]
    fn detect_boundaries_finds_the_phrase_cut() {
        assert!(
            !detect_boundaries(&two_phrases(), 0).is_empty(),
            "two_phrases has a detectable boundary"
        );
    }

    #[test]
    fn capture_codes_map_in_cli_order() {
        assert_eq!(rights_status_from(0), RightsStatus::PublicDomain);
        assert_eq!(rights_status_from(99), RightsStatus::CopyrightedComposition);
        assert_eq!(acquisition_from(2), Acquisition::SelfTranscribed);
        assert_eq!(reviewer_from(1), Some(ReviewerDecision::Rejected));
        assert_eq!(reviewer_from(99), None);
        assert_eq!(cohort_from(1), StyleCohort::Adjacent);
        assert_eq!(cohort_from(0), StyleCohort::Core);
    }
}
