//! Corpus annotation schema (S5).
//!
//! Defines `ChunkMeta` and `CorpusManifest` — the serialisable types that
//! describe every phrase chunk committed to the corpus.  Corpus *content* is
//! git-ignored; only the schema, tooling, and minimal fixtures live in the
//! repository (ADR-0005 / licensing).

use serde::{Deserialize, Serialize};

use crate::complement::AxisScores;
use crate::gesture::GestureStats;
use crate::structure::{ComplexityProfile, StructureMetrics};

/// Current corpus schema version.
///
/// - v1 — the S5 baseline schema.
/// - v2 — S14 Phase 3: `ChunkMeta` gains optional measured
///   [`StructureMetrics`]; v1 records (no `structure` key) keep loading and
///   re-serialize losslessly.
/// - v3 — burst/rest gesture statistics (melodic-closure note §7.4):
///   `ChunkMeta` gains optional measured [`GestureStats`] under the same
///   pattern; v1/v2 records (no `gesture` key) keep loading and re-serialize
///   losslessly.
/// - v4 — style cohort + ensemble groups (decisions 2026-06-11): `ChunkMeta`
///   gains optional [`StyleCohort`] and [`EnsembleRef`] under the same
///   pattern, and [`CorpusManifest`] gains `groups` (skipped while empty), so
///   pre-v4 records and manifests keep loading and re-serialize losslessly.
/// - v5 — sub-bar period detection (S14 refinement): [`StructureMetrics`]
///   gains optional `detected_subbar_period_ticks` under the same pattern;
///   pre-v5 structure blocks load it as `None` and re-serialize losslessly.
/// - v6 — the per-axis complexity profile (S14): `ChunkMeta` gains optional
///   measured [`ComplexityProfile`] under the same pattern; pre-v6 records
///   load it as `None` and re-serialize losslessly.
pub const SCHEMA_VERSION: u32 = 6;

// ── identifiers ───────────────────────────────────────────────────────────────

/// Unique, stable identifier for a corpus chunk (e.g. `"dgd_001"`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ChunkId(pub String);

// ── source provenance ─────────────────────────────────────────────────────────

/// The import format a chunk was sourced from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceFormat {
    Midi,
    Gp3,
    Gp4,
    Gp5,
    Gpx,
}

/// A reference to the source file from which a chunk was extracted.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SourceRef {
    /// Basename of the source file (full path not stored for privacy).
    pub filename: String,
    /// Import format.
    pub format: SourceFormat,
    /// Inclusive `[first_bar, last_bar]` range within the source (0-indexed).
    pub bar_range: Option<(u32, u32)>,
}

// ── swancore tag taxonomy ─────────────────────────────────────────────────────

/// Swancore style/technique tags for a corpus chunk (ADR-0005).
///
/// The taxonomy covers style, harmony, technique, rhythm, and structure.
/// A chunk typically carries 2–6 tags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SwancoreTag {
    // ── style ──────────────────────────────────────────────────────────────
    /// Clean, articulate single-note riff.
    CleanRiff,
    /// Heavily syncopated riff with off-beat accents.
    SyncopatedRiff,
    /// Passage dominated by two-hand tapping.
    TappingPassage,
    /// Passage dominated by legato (hammer-on/pull-off) runs.
    LegatoPassage,
    // ── harmony ────────────────────────────────────────────────────────────
    /// Major 7th chord voicing.
    Maj7,
    /// Minor 7th chord voicing.
    Min7,
    /// Suspended 2nd chord voicing.
    Sus2,
    /// Added 9th chord voicing.
    Add9,
    /// Slash chord (e.g. G/B).
    SlashChord,
    /// Power chord (5th dyad).
    PowerChord,
    // ── technique ──────────────────────────────────────────────────────────
    HammerOn,
    PullOff,
    Slide,
    Bend,
    Vibrato,
    PalmMute,
    NaturalHarmonic,
    ArtificialHarmonic,
    // ── rhythm ─────────────────────────────────────────────────────────────
    /// Off-beat / displaced phrasing.
    Syncopated,
    /// Swing or triplet-based feel.
    TripletFeel,
    /// Cross-rhythm or polyrhythmic writing.
    Polyrhythm,
    // ── structure ──────────────────────────────────────────────────────────
    Intro,
    Verse,
    Chorus,
    Bridge,
    Outro,
    Interlude,
}

impl SwancoreTag {
    /// All variants in display order, used by the curation CLI.
    pub const fn all_variants() -> &'static [Self] {
        &[
            Self::CleanRiff,
            Self::SyncopatedRiff,
            Self::TappingPassage,
            Self::LegatoPassage,
            Self::Maj7,
            Self::Min7,
            Self::Sus2,
            Self::Add9,
            Self::SlashChord,
            Self::PowerChord,
            Self::HammerOn,
            Self::PullOff,
            Self::Slide,
            Self::Bend,
            Self::Vibrato,
            Self::PalmMute,
            Self::NaturalHarmonic,
            Self::ArtificialHarmonic,
            Self::Syncopated,
            Self::TripletFeel,
            Self::Polyrhythm,
            Self::Intro,
            Self::Verse,
            Self::Chorus,
            Self::Bridge,
            Self::Outro,
            Self::Interlude,
        ]
    }
}

// ── quality and curation ──────────────────────────────────────────────────────

/// Quality flags recorded during curation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QualityFlag {
    /// Clean import with no warnings.
    Clean,
    /// Import emitted one or more `LossReport` warnings.
    Lossy,
    /// Timing appears heavily quantized.
    Quantized,
    /// Velocity data is missing or uniform.
    FlatDynamics,
}

/// Curator decision on whether a chunk should be used for training / eval.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewerDecision {
    Accepted,
    Rejected,
    NeedsReview,
}

// ── boundary summary ──────────────────────────────────────────────────────────

/// A phrase boundary embedded in chunk metadata.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct BoundaryEntry {
    pub start_tick: u32,
    pub end_tick: u32,
    /// Boundary detector confidence score in `[0.0, 1.0]`.
    pub score: f64,
}

// ── style cohort and ensemble groups (schema v4) ──────────────────────────────

/// Style cohort of a chunk relative to the swancore-first scope
/// (decisions 2026-06-11: per-consumer corpus slices).
///
/// Statistical gates and the style centroid read the `Core` slice only; the
/// graph layer reads the full corpus with per-cohort transition statistics;
/// novelty references and taste ignore cohorts. `None` (pre-v4 records) means
/// *unlabeled* — slice policies decide how to treat it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StyleCohort {
    /// Core swancore material.
    Core,
    /// Adjacent-genre material admitted for coverage and graph mass.
    Adjacent,
}

/// Link from a chunk to its ensemble group: one source span, several parts
/// (e.g. the two role-fluid guitars of a DGD section), each curated as its
/// own single-part chunk.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EnsembleRef {
    /// Group identifier shared by sibling chunks (e.g. `"dgd_042"`).
    pub group_id: String,
    /// Zero-based part index within the group.
    pub part_index: u32,
}

/// Measured relation between two parts of an ensemble group — the corpus-side
/// complement hyperedge fact (glossary §9), persisted at curation time.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PairRelation {
    /// Part indices `(a, b)` with `a < b`; axes read *b relative to a*
    /// (`density_ratio` orientation of `measure_pair_axes`).
    pub parts: (u32, u32),
    /// Measured relation axes (complement vocabulary, ADR-0012/0017).
    pub axes: AxisScores,
}

/// An ensemble group: several single-part chunks curated from one source
/// span, plus their measured pairwise relations. No role labels by design —
/// the per-phrase relation axes carry the role information.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EnsembleGroup {
    /// Group identifier (shared with the members' [`EnsembleRef`]s).
    pub id: String,
    /// Member chunk ids, ordered by part index.
    pub members: Vec<ChunkId>,
    /// Measured pairwise relations, ordered by `(a, b)`.
    pub relations: Vec<PairRelation>,
}

// ── chunk metadata ────────────────────────────────────────────────────────────

/// Full annotation for one corpus chunk.
///
/// All fields are required except the two curation-state options: `reviewer`
/// is `None` until a curator has decided, and `structure` is `None` for v1
/// records predating the schema-v2 bump (S14 Phase 3).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChunkMeta {
    pub id: ChunkId,
    pub title: String,
    pub source: SourceRef,
    /// Tempo in BPM at the start of the chunk.
    pub tempo_bpm: f64,
    pub ticks_per_quarter: u16,
    /// `(numerator, denominator)` time signature.
    pub time_signature: (u8, u8),
    /// Tuning string (e.g. `"standard_e"`, `"drop_d"`).
    pub tuning: String,
    pub tags: Vec<SwancoreTag>,
    pub boundaries: Vec<BoundaryEntry>,
    /// Named techniques present in the chunk (free-form, `lower_snake_case`).
    pub techniques: Vec<String>,
    pub quality_flags: Vec<QualityFlag>,
    pub reviewer: Option<ReviewerDecision>,
    /// Measured structural metrics of the chunk's first note-bearing track
    /// (S14 Phase 3, schema v2). Absent in v1 records; skipped — not written
    /// as `null` — when unmeasured, so v1 files round-trip byte-identically.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub structure: Option<StructureMetrics>,
    /// Measured burst/rest gesture statistics of the same track (schema v3,
    /// melodic-closure note §7.4). Absent in v1/v2 records; skipped — not
    /// written as `null` — when unmeasured, so older files round-trip
    /// byte-identically.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gesture: Option<GestureStats>,
    /// Measured per-axis complexity profile (schema v6). Absent — not stored
    /// as `null` — when unmeasured, so pre-v6 files round-trip byte-identically.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub complexity: Option<ComplexityProfile>,
    /// Style cohort (schema v4). Absent in pre-v4 records — unlabeled; the
    /// key is skipped when unset, so older files round-trip byte-identically.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style_cohort: Option<StyleCohort>,
    /// Ensemble-group link (schema v4) for chunks curated as one part of a
    /// multi-guitar phrase. Absent for standalone chunks and pre-v4 records;
    /// the key is skipped when unset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ensemble: Option<EnsembleRef>,
    /// ISO 8601 creation timestamp.
    pub created_at: String,
    /// ISO 8601 last-modified timestamp.
    pub updated_at: String,
}

// ── manifest ──────────────────────────────────────────────────────────────────

/// Top-level corpus manifest: a versioned list of all chunk metadata records.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CorpusManifest {
    /// Monotonically increasing schema version (current: [`SCHEMA_VERSION`]).
    pub schema_version: u32,
    pub chunks: Vec<ChunkMeta>,
    /// Ensemble groups over the chunks (schema v4). The key is skipped while
    /// empty, so pre-v4 manifests keep loading and re-serialize losslessly.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub groups: Vec<EnsembleGroup>,
}
