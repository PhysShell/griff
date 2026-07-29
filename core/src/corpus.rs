//! Corpus annotation schema (S5).
//!
//! Defines `ChunkMeta` and `CorpusManifest` — the serialisable types that
//! describe every phrase chunk committed to the corpus.  Corpus *content* is
//! git-ignored; only the schema, tooling, and minimal fixtures live in the
//! repository (ADR-0005 / licensing).

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::complement::AxisScores;
use crate::gesture::GestureStats;
use crate::novelty::PhraseDuplicate;
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
/// - v7 — per-chunk rights + provenance (decisions 2026-06-12): `ChunkMeta`
///   gains optional [`RightsInfo`] under the same pattern; pre-v7 records (no
///   `rights` key) keep loading and re-serialize losslessly. Rights status is
///   not derivable from content, so it is captured at curation time and cannot
///   be backfilled.
/// - v8 — persisted near-duplicate link (#76): `ChunkMeta` gains optional
///   [`PhraseDuplicate`] under the same pattern; pre-v8 records (no `duplicate`
///   key) keep loading and re-serialize losslessly. It records which earlier
///   split phrase a later one repeats — a curation signal previously surfaced
///   only in the UI/CLI and dropped on download.
/// - v9 — exact source track + integrity hash: [`SourceRef`] gains optional
///   `track_index` and `sha256` under the same pattern. Bulk multi-track ingest
///   needs the exact track (a second-guitar chunk must not reload as the first)
///   and a content hash (a filename is not an identity). Pre-v9 records lack
///   both and keep the legacy first-note-bearing, unverified behavior; the keys
///   are skipped when unset, so older files round-trip byte-identically.
/// - v10 — canonical song identity (ADR-0031): [`SourceRef`] gains optional
///   `song_id` (a curator-assigned Work identifier above the `sha256`
///   Manifestation) and [`CorpusManifest`] gains an optional `songs` map, under
///   the same pattern. This is the identity that makes song-level holdout
///   implementable fail-closed. Pre-v10 records lack the key, load it as `None`,
///   and re-serialize byte-identically.
///
/// Tag taxonomy is intentionally *not* versioned here: [`SwancoreTag`] grows
/// additively (e.g. `let_ring`, #75) and `SCHEMA_VERSION` tracks structural
/// `ChunkMeta` changes (the optional-field, forward-compatible pattern above),
/// not the tag set — a new tag only breaks readers that hard-reject unknown
/// variants, a curation-tooling concern, not a corpus-structure one
/// (decisions 2026-06-19).
pub const SCHEMA_VERSION: u32 = 10;

// ── identifiers ───────────────────────────────────────────────────────────────

/// Unique, stable identifier for a corpus chunk (e.g. `"dgd_001"`).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ChunkId(pub String);

/// Stable, opaque, curator-assigned identifier of a **composition** — a Work
/// (schema v10, ADR-0031).
///
/// It sits one level above [`SourceRef::sha256`]: a `sha256` identifies one
/// source *file* (a Manifestation), and a `SongId` names the work that several
/// files transcribe. It is a fact about provenance the curator asserts, never
/// derived from content similarity, and it grants no scoring authority — its
/// sole purpose is leakage-safe song-level holdout and source-identity splits.
/// Every manifestation, arrangement, edition, and cover of one composition
/// carries the **same** `SongId`; musical distinctness never changes Work
/// identity.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SongId(pub String);

/// Lowercase-hex SHA-256 of a source file's bytes — the content identity a
/// filename cannot provide (schema v9 [`SourceRef::sha256`]). Shared by the
/// ingest that records it and the loader that verifies it.
#[must_use]
pub fn source_sha256(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    use std::fmt::Write as _;
    Sha256::digest(bytes)
        .iter()
        .fold(String::new(), |mut acc, byte| {
            write!(acc, "{byte:02x}").ok();
            acc
        })
}

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
    /// Guitar Pro 7/8 (`.gp`).
    Gp,
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
    /// Which source track this chunk was cut from (schema v9). Absent in pre-v9
    /// records, where the loader falls back to the first note-bearing track —
    /// safe for single-track `griff split`, but a multi-track bulk ingest must
    /// name the exact track so a second-guitar chunk is never reloaded as the
    /// first. When present, the loader uses exactly this track and fails rather
    /// than substitute another. The key is skipped when unset, so pre-v9 files
    /// round-trip byte-identically.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub track_index: Option<u32>,
    /// SHA-256 of the source file's bytes (schema v9), lowercase hex. Pins the
    /// material: `filename` is a human name, not an identity, so the loader
    /// verifies this before trusting a same-named file. Absent in pre-v9
    /// records (unverified, as before); skipped when unset for byte-identical
    /// round-trip.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sha256: Option<String>,
    /// Curator-assigned canonical song identity (schema v10, ADR-0031) — the
    /// Work above this file. Two source files of one composition share a
    /// `song_id` even though their `sha256` differ, which is what makes
    /// song-level holdout implementable fail-closed. Absent in pre-v10 records —
    /// the key is skipped when unset, so older files round-trip byte-identically.
    /// Never backfilled by analysis; a `None` here means "uncurated", and the
    /// default song-holdout preflight refuses rather than guess.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub song_id: Option<SongId>,
}

// ── swancore tag taxonomy ─────────────────────────────────────────────────────

/// Swancore style/technique tags for a corpus chunk (ADR-0005).
///
/// The taxonomy covers style, harmony, technique, rhythm, and structure.
/// A chunk typically carries 2–6 tags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
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
    /// Let ring — notes left to sustain into following beats.
    LetRing,
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
            Self::LetRing,
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

// ── rights and provenance (schema v7) ─────────────────────────────────────────

/// Rights status of a chunk's underlying material (decisions 2026-06-12).
///
/// Not an OSS licence — this records the *composition* rights status, paired
/// with [`Acquisition`] provenance. Most scraped, purchased, or self-transcribed
/// modern-metal tabs are `CopyrightedComposition`; only public-domain sources
/// (e.g. PDMX `MusicXML`) are freely redistributable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RightsStatus {
    /// Public-domain composition (e.g. PDMX `MusicXML`).
    PublicDomain,
    /// Creative Commons Attribution.
    CcBy,
    /// Creative Commons Attribution-ShareAlike.
    CcBySa,
    /// Composition under copyright (the common case for modern-metal tabs).
    CopyrightedComposition,
    /// Rights status not yet determined.
    Unknown,
}

/// How a chunk's source file was acquired (decisions 2026-06-12).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Acquisition {
    /// Scraped from a community tab site (Ultimate Guitar, Songsterr, …).
    CommunityTabSite,
    /// Purchased from an official publisher (e.g. Sheet Happens GP).
    PurchasedOfficial,
    /// Transcribed by the curator (own transcription does not transfer the
    /// underlying composition rights).
    SelfTranscribed,
    /// Optical music recognition from a scan.
    OmrFromScan,
    /// Provided directly by the artist.
    ArtistProvided,
}

/// Per-chunk rights and provenance (schema v7, decisions 2026-06-12).
///
/// `redistributable` is a typed fact — not a freeform note — because
/// `novelty.rs` and any future export gate must filter on it without scanning
/// prose. Captured at curation time: rights status cannot be derived from the
/// notes, so backfilling would mean re-researching provenance per source.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RightsInfo {
    pub rights_status: RightsStatus,
    pub acquisition: Acquisition,
    /// Whether the chunk's source material may be redistributed.
    pub redistributable: bool,
    /// Free-form provenance note (source URL, acquisition date, publisher).
    pub notes: String,
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
    /// Near-duplicate link (schema v8, #76): the earlier split phrase this one
    /// verbatim-quotes (transposition-aware) and by how much, or absent when the
    /// phrase is distinct or was captured outside a split. `of` indexes the
    /// phrase within the same split run — it pairs with the `_p<N>` id suffix.
    /// Skipped — not written as `null` — when absent, so pre-v8 files round-trip
    /// byte-identically.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duplicate: Option<PhraseDuplicate>,
    /// Style cohort (schema v4). Absent in pre-v4 records — unlabeled; the
    /// key is skipped when unset, so older files round-trip byte-identically.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style_cohort: Option<StyleCohort>,
    /// Ensemble-group link (schema v4) for chunks curated as one part of a
    /// multi-guitar phrase. Absent for standalone chunks and pre-v4 records;
    /// the key is skipped when unset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ensemble: Option<EnsembleRef>,
    /// Rights and provenance (schema v7, decisions 2026-06-12). Absent in pre-v7
    /// records — the key is skipped when unset, so older files round-trip
    /// byte-identically. Captured at curation time; cannot be backfilled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rights: Option<RightsInfo>,
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
    /// Canonical song map (schema v10, ADR-0031): `SongId → [sha256]`. A
    /// *convenience and typo check*, not the coverage proof — corpus-wide song
    /// coverage is proven by the per-`SourceRef` `song_id` scan in
    /// [`song_holdout_preflight`], which runs whether or not this map is present.
    ///
    /// `None` (the key absent) means "no manifest supplied"; `Some` (even an
    /// empty `{}`) means "a manifest is present" and triggers the bidirectional
    /// agreement check — an explicitly-present empty map must therefore account
    /// for every labelled source, so absent and present-empty are **not** the
    /// same. The key is skipped while `None`, so pre-v10 manifests round-trip
    /// byte-identically.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub songs: Option<BTreeMap<SongId, Vec<String>>>,
}

// ── song-level holdout preflight (schema v10, ADR-0031) ─────────────────────────

/// A reason [`song_holdout_preflight`] refuses a corpus for song-level holdout.
///
/// Song holdout is **fail-closed**: any uncurated, unidentifiable, or
/// inconsistent source is a typed refusal, never a silent pick (ADR-0031). All
/// refusals are collected so one preflight reports every problem.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum SongHoldoutRefusal {
    /// A chunk carries no `sha256`, so its source file cannot be identified —
    /// song holdout cannot prove it is not an alternate transcription of the
    /// held-out song.
    UnidentifiedSource { chunk: ChunkId },
    /// A participating source file (`sha256`) carries no `song_id`. The default
    /// policy refuses rather than guess; carries an example chunk that cites it.
    UncuratedSource { sha256: String, example: ChunkId },
    /// One source file (`sha256`) is labelled with more than one `SongId` across
    /// its chunks — a file split across works would leak part of it. `song_ids`
    /// is sorted and deduplicated.
    InconsistentSource {
        sha256: String,
        song_ids: Vec<SongId>,
    },
    /// The `songs` manifest lists a `(song_id, sha256)` pair no source carries.
    ManifestSourceMissing { song_id: SongId, sha256: String },
    /// A source labelled with `song_id` whose `sha256` the present `songs`
    /// manifest does not enumerate under that id.
    ManifestLabelMissing { sha256: String, song_id: SongId },
}

/// Validate a corpus for fail-closed song-level holdout (ADR-0031).
///
/// Returns `Ok(())` only when the whole corpus may be song-held-out without
/// leakage; otherwise every refusal is collected. This is a preflight over the
/// entire manifest — every chunk participates — and it is read-only (it never
/// backfills `song_id`). It checks, in order:
///
/// 1. every participating chunk has a `sha256` (an unidentifiable source cannot
///    be reasoned about);
/// 2. every participating source (`sha256`) carries a `song_id` — the
///    strict-refusal default, since a `None` may be an alternate transcription
///    of the held-out work;
/// 3. each `sha256` maps to exactly one `SongId` (no file split across works);
/// 4. if the `songs` manifest is present, it agrees with the per-source labels
///    **both ways** (every manifest pair has a matching source, every labelled
///    source is enumerated).
pub fn song_holdout_preflight(manifest: &CorpusManifest) -> Result<(), Vec<SongHoldoutRefusal>> {
    let mut refusals = Vec::new();
    let labels = collect_source_labels(&manifest.chunks, &mut refusals);
    check_source_consistency(&labels, &mut refusals);
    // A present manifest — even an explicitly empty `{}` — is checked both ways;
    // only an absent (`None`) manifest skips the cross-check.
    if let Some(songs) = &manifest.songs {
        check_manifest_agreement(songs, &labels, &mut refusals);
    }
    if refusals.is_empty() {
        Ok(())
    } else {
        Err(refusals)
    }
}

/// What the chunks assert about one source file (`sha256`): the distinct
/// `SongId`s labelling it, whether any chunk left it unlabelled, and example
/// chunk ids for messages.
#[derive(Debug)]
struct SourceLabel {
    songs: BTreeSet<SongId>,
    example: ChunkId,
    none_example: Option<ChunkId>,
}

/// Group chunks by source `sha256`, recording each file's `song_id` labels. A
/// chunk with no `sha256` cannot be file-identified, so it is refused outright.
fn collect_source_labels(
    chunks: &[ChunkMeta],
    refusals: &mut Vec<SongHoldoutRefusal>,
) -> BTreeMap<String, SourceLabel> {
    let mut labels: BTreeMap<String, SourceLabel> = BTreeMap::new();
    for chunk in chunks {
        let Some(sha) = &chunk.source.sha256 else {
            refusals.push(SongHoldoutRefusal::UnidentifiedSource {
                chunk: chunk.id.clone(),
            });
            continue;
        };
        let entry = labels.entry(sha.clone()).or_insert_with(|| SourceLabel {
            songs: BTreeSet::new(),
            example: chunk.id.clone(),
            none_example: None,
        });
        match &chunk.source.song_id {
            Some(song) => {
                entry.songs.insert(song.clone());
            }
            None => {
                if entry.none_example.is_none() {
                    entry.none_example = Some(chunk.id.clone());
                }
            }
        }
    }
    labels
}

/// Refuse any source that is uncurated (no `song_id`, the strict-refusal
/// default) or split across more than one `SongId` (chunk-level leakage).
fn check_source_consistency(
    labels: &BTreeMap<String, SourceLabel>,
    refusals: &mut Vec<SongHoldoutRefusal>,
) {
    for (sha, label) in labels {
        if let Some(example) = &label.none_example {
            refusals.push(SongHoldoutRefusal::UncuratedSource {
                sha256: sha.clone(),
                example: example.clone(),
            });
        } else if label.songs.is_empty() {
            // Every chunk of this file lacked a song_id: still uncurated.
            refusals.push(SongHoldoutRefusal::UncuratedSource {
                sha256: sha.clone(),
                example: label.example.clone(),
            });
        }
        if label.songs.len() >= 2 {
            refusals.push(SongHoldoutRefusal::InconsistentSource {
                sha256: sha.clone(),
                song_ids: label.songs.iter().cloned().collect(),
            });
        }
    }
}

/// When the `songs` manifest is present, require it to agree with the per-source
/// labels both ways: no dangling manifest pair, no unenumerated labelled source.
fn check_manifest_agreement(
    songs: &BTreeMap<SongId, Vec<String>>,
    labels: &BTreeMap<String, SourceLabel>,
    refusals: &mut Vec<SongHoldoutRefusal>,
) {
    let mut source_pairs: BTreeSet<(SongId, String)> = BTreeSet::new();
    for (sha, label) in labels {
        for song in &label.songs {
            source_pairs.insert((song.clone(), sha.clone()));
        }
    }
    let mut manifest_pairs: BTreeSet<(SongId, String)> = BTreeSet::new();
    for (song, shas) in songs {
        for sha in shas {
            manifest_pairs.insert((song.clone(), sha.clone()));
        }
    }
    for (song, sha) in manifest_pairs.difference(&source_pairs) {
        refusals.push(SongHoldoutRefusal::ManifestSourceMissing {
            song_id: song.clone(),
            sha256: sha.clone(),
        });
    }
    for (song, sha) in source_pairs.difference(&manifest_pairs) {
        refusals.push(SongHoldoutRefusal::ManifestLabelMissing {
            sha256: sha.clone(),
            song_id: song.clone(),
        });
    }
}
