//! Corpus annotation schema (S5).
//!
//! Defines `ChunkMeta` and `CorpusManifest` — the serialisable types that
//! describe every phrase chunk committed to the corpus.  Corpus *content* is
//! git-ignored; only the schema, tooling, and minimal fixtures live in the
//! repository (ADR-0005 / licensing).

use serde::{Deserialize, Serialize};

use crate::gesture::GestureStats;
use crate::structure::StructureMetrics;

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
pub const SCHEMA_VERSION: u32 = 3;

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
}
