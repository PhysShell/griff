//! Corpus annotation schema (S5).
//!
//! Defines `ChunkMeta` and `CorpusManifest` — the serialisable types that
//! describe every phrase chunk committed to the corpus.  Corpus *content* is
//! git-ignored; only the schema, tooling, and minimal fixtures live in the
//! repository (ADR-0005 / licensing).

use serde::{Deserialize, Serialize};

use crate::structure::StructureMetrics;

/// Current corpus schema version (ADR-0015 Phase 3: `ChunkMeta` gained the
/// optional `structure` snapshot; v1 records without it still parse).
pub const CORPUS_SCHEMA_VERSION: u32 = 2;

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

// ── structure snapshot (schema v2) ────────────────────────────────────────────

/// The persisted form of measured [`StructureMetrics`] on a chunk
/// (S14 Phase 3, ADR-0015 §3).
///
/// A snapshot of what the chunk's material *is* — detected pattern period,
/// repeatability, variation, loopability, structural complexity — computed at
/// curation time. Decoupled from the analysis struct so the corpus schema does
/// not chase its refinements (sub-bar periods, the complexity vector); later it
/// also feeds S7 node attributes.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct StructureSnapshot {
    /// Number of master bars analysed.
    pub bar_count: usize,
    /// Detected pattern period in bars, or `None` if through-composed.
    pub pattern_period_bars: Option<usize>,
    /// The same period in ticks, or `None`.
    pub pattern_period_ticks: Option<u32>,
    /// Measured repeatability, in `[0, 1]`.
    pub repeatability: f64,
    /// Measured variation (`1 − repeatability`), in `[0, 1]`.
    pub variation: f64,
    /// Measured loop-seam smoothness, in `[0, 1]`.
    pub loopability: f64,
    /// Fraction of distinct bar signatures, in `[0, 1]`.
    pub structural_complexity: f64,
}

impl From<StructureMetrics> for StructureSnapshot {
    fn from(m: StructureMetrics) -> Self {
        Self {
            bar_count: m.bar_count,
            pattern_period_bars: m.detected_pattern_period_bars,
            pattern_period_ticks: m.detected_pattern_period_ticks,
            repeatability: m.repeatability_score,
            variation: m.variation_score,
            loopability: m.loopability_score,
            structural_complexity: m.structural_complexity,
        }
    }
}

// ── chunk metadata ────────────────────────────────────────────────────────────

/// Full annotation for one corpus chunk.
///
/// All fields are required; `reviewer` is `None` until a curator has decided.
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
    /// Measured structure snapshot (schema v2, S14 Phase 3). Omitted from the
    /// JSON when absent, so unanalysed chunks keep the v1 shape and v1 records
    /// parse as `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub structure: Option<StructureSnapshot>,
    /// ISO 8601 creation timestamp.
    pub created_at: String,
    /// ISO 8601 last-modified timestamp.
    pub updated_at: String,
}

// ── manifest ──────────────────────────────────────────────────────────────────

/// Top-level corpus manifest: a versioned list of all chunk metadata records.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CorpusManifest {
    /// Monotonically increasing schema version (current: [`CORPUS_SCHEMA_VERSION`]).
    pub schema_version: u32,
    pub chunks: Vec<ChunkMeta>,
}
