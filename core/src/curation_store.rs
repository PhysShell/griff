//! Curation store — the backend-neutral domain model for human curation
//! decisions (step 3B1).
//!
//! Pure: types, a versioned wire format (`encode_store`/`decode_store`),
//! validation, a per-chunk projection, and reconciliation against a corpus.
//! **No I/O** — no filesystem, no temp files, no rename, no OPFS. A backend
//! (3B2 native, 3C OPFS) owns reading and writing bytes and generating opaque
//! ids; this module owns the format and its rules.
//!
//! A decision is an **append-only event**, keyed by a stable [`ChunkId`], not a
//! position, cluster, or representative. The latest decision for a chunk is the
//! event with the greatest `(occurred_at, event_id)`. Cluster context is an
//! audit snapshot on the event, never part of the decision's identity — the
//! representative can change when dedup provenance is corrected without a human
//! decision moving or vanishing.

use std::collections::{BTreeMap, HashSet};
use std::ops::Range;

use serde::{Deserialize, Serialize};

use crate::corpus::{
    source_sha256, ChunkId, ChunkMeta, ReviewerDecision, SwancoreTag, SCHEMA_VERSION,
};

/// The one wire version this module reads and writes.
pub const CURATION_STORE_VERSION: u32 = 1;

/// An opaque decision-event id, minted by the boundary layer (not by core).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct CurationEventId(pub String);

/// An opaque reviewer identity.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ReviewerId(pub String);

/// A hash pinning a whole corpus snapshot's pinned identity.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct CorpusFingerprint(pub String);

/// A hash pinning one chunk's material identity.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct ChunkFingerprint(pub String);

/// The dedup-cluster context a decision was made in — an audit snapshot, never
/// part of the decision's identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CurationContext {
    /// The cluster's representative at decision time.
    pub cluster_representative: ChunkId,
    /// The cluster's members at decision time.
    pub cluster_members: Vec<ChunkId>,
}

/// One curation decision event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CurationEvent {
    /// Opaque, unique id (from the boundary).
    pub event_id: CurationEventId,
    /// The chunk this decision is about — the decision's identity.
    pub chunk_id: ChunkId,
    /// The chunk's pinned fingerprint at decision time.
    pub chunk_fingerprint: ChunkFingerprint,
    /// The decision.
    pub decision: ReviewerDecision,
    /// Who decided.
    pub reviewer: ReviewerId,
    /// Canonical UTC timestamp `YYYY-MM-DDTHH:MM:SSZ` (second precision, no
    /// fractional part); validated on both encode and decode.
    pub occurred_at: String,
    /// The corpus fingerprint the decision was made against.
    pub corpus_fingerprint: CorpusFingerprint,
    /// The cluster context snapshot (audit only).
    pub context: CurationContext,
    /// Curatorial tags applied (canonicalized: sorted, deduped).
    pub tags: Vec<SwancoreTag>,
    /// Optional free-form note.
    pub note: Option<String>,
}

/// The versioned store envelope. `version` is explicit — the format never
/// guesses its own shape from which fields are present.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CurationStoreV1 {
    /// Wire version; must equal [`CURATION_STORE_VERSION`].
    pub version: u32,
    /// The corpus the events were recorded against.
    pub corpus_fingerprint: CorpusFingerprint,
    /// The decision events (canonical order after decode).
    pub events: Vec<CurationEvent>,
}

/// A store that parses (or is about to be written) but breaks the domain rules.
///
/// The *same* checks guard both directions: [`decode_store`] runs them on bytes
/// it reads, [`encode_store`] runs them before it writes — so invalid state can
/// never be persisted and then only discovered on the next load.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreValidationError {
    /// The `version` field is not one this module understands.
    UnsupportedVersion { found: u32 },
    /// The envelope's `corpus_fingerprint` is empty.
    EmptyCorpusFingerprint,
    /// Two events share an id.
    DuplicateEventId { id: CurationEventId },
    /// A required opaque field was empty.
    EmptyField {
        field: &'static str,
        event: CurationEventId,
    },
    /// `occurred_at` is not a canonical UTC timestamp.
    InvalidTimestamp {
        event: CurationEventId,
        value: String,
    },
}

/// Why a store's bytes could not be decoded into a valid store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreDecodeError {
    /// The bytes are not valid store JSON. Never treated as an empty store.
    MalformedStore(String),
    /// The bytes parse but the store violates a domain rule.
    Invalid(StoreValidationError),
}

/// Why a store could not be encoded to bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreEncodeError {
    /// The store violates a domain rule; refused before any bytes are produced,
    /// so a write can never persist invalid state — or a truncated/empty file.
    Invalid(StoreValidationError),
    /// Serialization itself failed. Unreachable for these plain types, but never
    /// swallowed into empty bytes: an empty file reads as "no decisions".
    Serialize(String),
}

/// Why a fingerprint could not be computed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FingerprintError {
    /// The chunk lacks the pinned identity (`sha256` / `track_index` /
    /// `bar_range`) a durable decision must bind to; a filename is not an
    /// identity, and a partial pin is not one either.
    UnpinnedChunk { chunk_id: ChunkId },
}

/// The projected latest decision for one chunk (cluster context excluded).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectedDecision {
    pub event_id: CurationEventId,
    pub decision: ReviewerDecision,
    pub reviewer: ReviewerId,
    pub occurred_at: String,
    pub tags: Vec<SwancoreTag>,
    pub note: Option<String>,
}

/// The current curation state: the latest decision per chunk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectedCuration {
    pub by_chunk: BTreeMap<ChunkId, ProjectedDecision>,
}

/// Why a stored event no longer applies to the current corpus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrphanReason {
    /// The chunk is gone from the current corpus.
    MissingChunk,
    /// The chunk exists but its pinned material changed.
    ChangedChunkFingerprint,
}

/// A stored event that does not apply to the current corpus, with the reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OrphanedEvent {
    pub event: CurationEvent,
    pub reason: OrphanReason,
}

/// The result of reconciling a store against the current corpus.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReconciledCuration {
    /// Events whose chunk is present and unchanged.
    pub active: Vec<CurationEvent>,
    /// Events that no longer apply, with why.
    pub orphaned: Vec<OrphanedEvent>,
    /// Whether the store's corpus fingerprint matches the current corpus.
    pub corpus_match: bool,
}

// ── fingerprints ────────────────────────────────────────────────────────────

/// The pinned material fingerprint of one chunk.
///
/// `ChunkId` + `sha256` + `track_index` + `bar_range`, hashed. All three pins
/// are required: a chunk missing any of `sha256`, `track_index`, or `bar_range`
/// is [`FingerprintError::UnpinnedChunk`]. There are no whole-track chunks —
/// every corpus chunk is a bar range — so a missing range is an incomplete pin,
/// not an identity, and there is no filename fallback.
#[allow(clippy::missing_errors_doc)]
pub fn chunk_fingerprint(chunk: &ChunkMeta) -> Result<ChunkFingerprint, FingerprintError> {
    let unpinned = || FingerprintError::UnpinnedChunk {
        chunk_id: chunk.id.clone(),
    };
    let sha = chunk.source.sha256.as_deref().ok_or_else(unpinned)?;
    let track = chunk.source.track_index.ok_or_else(unpinned)?;
    let (first, last) = chunk.source.bar_range.ok_or_else(unpinned)?;
    let canonical = format!(
        "chunk-v9\nid={}\nsha256={sha}\ntrack={track}\nbars={first}-{last}",
        chunk.id.0
    );
    Ok(ChunkFingerprint(source_sha256(canonical.as_bytes())))
}

/// The corpus fingerprint: the schema version plus the sorted set of chunk
/// fingerprints, hashed. Independent of chunk order and of any curator-mutable
/// field (tags, reviewer, timestamps).
#[allow(clippy::missing_errors_doc)]
pub fn corpus_fingerprint(
    schema_version: u32,
    chunks: &[ChunkMeta],
) -> Result<CorpusFingerprint, FingerprintError> {
    let mut fingerprints: Vec<String> = chunks
        .iter()
        .map(|c| chunk_fingerprint(c).map(|f| f.0))
        .collect::<Result<_, _>>()?;
    fingerprints.sort();
    let canonical = format!(
        "corpus-v9\nschema={schema_version}\n{}",
        fingerprints.join("\n")
    );
    Ok(CorpusFingerprint(source_sha256(canonical.as_bytes())))
}

// ── encode / decode ─────────────────────────────────────────────────────────

/// Serializes a store into its one canonical byte form.
///
/// The store is canonicalized (events and tags) and then validated with exactly
/// the same rules [`decode_store`] applies, so a caller can never write bytes
/// that would fail to load. Nothing is ever swallowed into empty output.
#[allow(clippy::missing_errors_doc)]
pub fn encode_store(store: &CurationStoreV1) -> Result<Vec<u8>, StoreEncodeError> {
    let mut canonical = store.clone();
    canonicalize(&mut canonical);
    validate_store(&canonical).map_err(StoreEncodeError::Invalid)?;
    serde_json::to_vec(&canonical).map_err(|e| StoreEncodeError::Serialize(e.to_string()))
}

/// Decodes and validates store bytes, canonicalizing the result. A malformed
/// store is a typed error, never an empty store.
#[allow(clippy::missing_errors_doc)]
pub fn decode_store(bytes: &[u8]) -> Result<CurationStoreV1, StoreDecodeError> {
    #[derive(Deserialize)]
    struct VersionProbe {
        version: u32,
    }
    // Read the explicit version first — the format never guesses its shape.
    let probe: VersionProbe = serde_json::from_slice(bytes)
        .map_err(|e| StoreDecodeError::MalformedStore(e.to_string()))?;
    if probe.version != CURATION_STORE_VERSION {
        return Err(StoreDecodeError::Invalid(
            StoreValidationError::UnsupportedVersion {
                found: probe.version,
            },
        ));
    }
    let mut store: CurationStoreV1 = serde_json::from_slice(bytes)
        .map_err(|e| StoreDecodeError::MalformedStore(e.to_string()))?;
    validate_store(&store).map_err(StoreDecodeError::Invalid)?;
    canonicalize(&mut store);
    Ok(store)
}

/// The domain rules a store must satisfy, independent of direction: a known
/// version, a non-empty envelope fingerprint, and, per event, non-empty opaque
/// fields, a canonical timestamp, and a unique id.
fn validate_store(store: &CurationStoreV1) -> Result<(), StoreValidationError> {
    if store.version != CURATION_STORE_VERSION {
        return Err(StoreValidationError::UnsupportedVersion {
            found: store.version,
        });
    }
    if store.corpus_fingerprint.0.is_empty() {
        return Err(StoreValidationError::EmptyCorpusFingerprint);
    }
    let mut seen: HashSet<&CurationEventId> = HashSet::new();
    for event in &store.events {
        let empty = |field: &'static str| StoreValidationError::EmptyField {
            field,
            event: event.event_id.clone(),
        };
        if event.event_id.0.is_empty() {
            return Err(empty("event_id"));
        }
        if event.chunk_id.0.is_empty() {
            return Err(empty("chunk_id"));
        }
        if event.chunk_fingerprint.0.is_empty() {
            return Err(empty("chunk_fingerprint"));
        }
        if event.corpus_fingerprint.0.is_empty() {
            return Err(empty("corpus_fingerprint"));
        }
        if event.reviewer.0.is_empty() {
            return Err(empty("reviewer"));
        }
        if !valid_timestamp(&event.occurred_at) {
            return Err(StoreValidationError::InvalidTimestamp {
                event: event.event_id.clone(),
                value: event.occurred_at.clone(),
            });
        }
        if !seen.insert(&event.event_id) {
            return Err(StoreValidationError::DuplicateEventId {
                id: event.event_id.clone(),
            });
        }
    }
    Ok(())
}

/// Canonical form: each event's tags sorted and deduped, events ordered by
/// `(occurred_at, event_id)` — so one logical store has one byte representation.
fn canonicalize(store: &mut CurationStoreV1) {
    for event in &mut store.events {
        event.tags.sort_unstable();
        event.tags.dedup();
    }
    store
        .events
        .sort_by(|a, b| (&a.occurred_at, &a.event_id).cmp(&(&b.occurred_at, &b.event_id)));
}

/// Whether `s` is the module's canonical UTC timestamp: exactly
/// `YYYY-MM-DDTHH:MM:SSZ` — second precision, **no fractional part** — with every
/// field in its real civil-calendar range (leap years included).
///
/// Fixed width and zero-padded is the point: for two canonical timestamps,
/// byte-lexicographic order equals chronological order, which is what lets
/// [`project`] use plain `String` comparison as a clock. A variable-width
/// fractional part would break that (`…00Z` would sort after `…00.5Z`), so it is
/// rejected rather than normalized.
fn valid_timestamp(s: &str) -> bool {
    // 2026-07-18T10:00:00Z — 20 ASCII bytes, fixed separators.
    let bytes = s.as_bytes();
    if bytes.len() != 20 {
        return false;
    }
    let sep = |i: usize, c: u8| bytes.get(i) == Some(&c);
    if !(sep(4, b'-')
        && sep(7, b'-')
        && sep(10, b'T')
        && sep(13, b':')
        && sep(16, b':')
        && sep(19, b'Z'))
    {
        return false;
    }
    let field = |r: Range<usize>| -> Option<u32> {
        let part = s.get(r)?;
        if part.bytes().all(|c| c.is_ascii_digit()) {
            part.parse().ok()
        } else {
            None
        }
    };
    let (Some(year), Some(month), Some(day), Some(hour), Some(min), Some(sec)) = (
        field(0..4),
        field(5..7),
        field(8..10),
        field(11..13),
        field(14..16),
        field(17..19),
    ) else {
        return false;
    };
    (1..=12).contains(&month)
        && (1..=days_in_month(year, month)).contains(&day)
        && hour <= 23
        && min <= 59
        && sec <= 59
}

/// Days in a civil-calendar month, proleptic-Gregorian leap rule. `0` for a
/// month outside `1..=12` (an out-of-range month fails its own check first).
const fn days_in_month(year: u32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if year.is_multiple_of(4) && (!year.is_multiple_of(100) || year.is_multiple_of(400)) => {
            29
        }
        2 => 28,
        _ => 0,
    }
}

// ── projection ──────────────────────────────────────────────────────────────

/// Projects events to the latest decision per chunk, by `(occurred_at,
/// event_id)`. Independent of input order; cluster context is not projected.
#[must_use]
pub fn project(events: &[CurationEvent]) -> ProjectedCuration {
    let mut latest: BTreeMap<&ChunkId, &CurationEvent> = BTreeMap::new();
    for event in events {
        let wins = latest.get(&event.chunk_id).is_none_or(|current| {
            (&event.occurred_at, &event.event_id) > (&current.occurred_at, &current.event_id)
        });
        if wins {
            latest.insert(&event.chunk_id, event);
        }
    }
    ProjectedCuration {
        by_chunk: latest
            .into_iter()
            .map(|(chunk, event)| {
                (
                    chunk.clone(),
                    ProjectedDecision {
                        event_id: event.event_id.clone(),
                        decision: event.decision,
                        reviewer: event.reviewer.clone(),
                        occurred_at: event.occurred_at.clone(),
                        tags: event.tags.clone(),
                        note: event.note.clone(),
                    },
                )
            })
            .collect(),
    }
}

// ── reconciliation ──────────────────────────────────────────────────────────

/// Reconciles a store's events against the current corpus.
///
/// An event stays active when its chunk is present with the same pinned
/// fingerprint (even if the shared corpus fingerprint moved, or the cluster
/// regrouped); it is orphaned when the chunk is gone or its material changed.
/// No fuzzy remap.
#[allow(clippy::missing_errors_doc)]
pub fn reconcile(
    store: &CurationStoreV1,
    current_chunks: &[ChunkMeta],
) -> Result<ReconciledCuration, FingerprintError> {
    let current_corpus = corpus_fingerprint(SCHEMA_VERSION, current_chunks)?;
    let mut current: BTreeMap<&str, ChunkFingerprint> = BTreeMap::new();
    for chunk in current_chunks {
        current.insert(chunk.id.0.as_str(), chunk_fingerprint(chunk)?);
    }

    let mut active = Vec::new();
    let mut orphaned = Vec::new();
    for event in &store.events {
        match current.get(event.chunk_id.0.as_str()) {
            None => orphaned.push(OrphanedEvent {
                event: event.clone(),
                reason: OrphanReason::MissingChunk,
            }),
            // The chunk's material is unchanged — active even if the shared
            // corpus fingerprint moved, or the cluster regrouped. No fuzzy remap.
            Some(fingerprint) if *fingerprint == event.chunk_fingerprint => {
                active.push(event.clone());
            }
            Some(_) => orphaned.push(OrphanedEvent {
                event: event.clone(),
                reason: OrphanReason::ChangedChunkFingerprint,
            }),
        }
    }
    Ok(ReconciledCuration {
        active,
        orphaned,
        corpus_match: store.corpus_fingerprint == current_corpus,
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used, clippy::indexing_slicing)]

    use super::{
        chunk_fingerprint, corpus_fingerprint, decode_store, encode_store, project, reconcile,
        ChunkFingerprint, CorpusFingerprint, CurationContext, CurationEvent, CurationEventId,
        CurationStoreV1, FingerprintError, OrphanReason, ReviewerId, StoreDecodeError,
        StoreEncodeError, StoreValidationError, CURATION_STORE_VERSION,
    };
    use crate::corpus::{
        ChunkId, ChunkMeta, ReviewerDecision, SourceFormat, SourceRef, SwancoreTag,
    };

    fn meta(
        id: &str,
        sha: Option<&str>,
        track: Option<u32>,
        bars: Option<(u32, u32)>,
    ) -> ChunkMeta {
        ChunkMeta {
            id: ChunkId(id.to_owned()),
            title: String::new(),
            source: SourceRef {
                filename: "s.gp".to_owned(),
                format: SourceFormat::Gp,
                bar_range: bars,
                track_index: track,
                sha256: sha.map(ToOwned::to_owned),
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
            structure: None,
            gesture: None,
            complexity: None,
            duplicate: None,
            style_cohort: None,
            ensemble: None,
            rights: None,
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    fn pinned(id: &str) -> ChunkMeta {
        meta(id, Some("hashof_"), Some(0), Some((0, 3)))
    }

    fn event(id: &str, chunk: &str, at: &str, fp: &str) -> CurationEvent {
        CurationEvent {
            event_id: CurationEventId(id.to_owned()),
            chunk_id: ChunkId(chunk.to_owned()),
            chunk_fingerprint: ChunkFingerprint(fp.to_owned()),
            decision: ReviewerDecision::Accepted,
            reviewer: ReviewerId("alice".to_owned()),
            occurred_at: at.to_owned(),
            corpus_fingerprint: CorpusFingerprint("corpus".to_owned()),
            context: CurationContext {
                cluster_representative: ChunkId(chunk.to_owned()),
                cluster_members: vec![ChunkId(chunk.to_owned())],
            },
            tags: Vec::new(),
            note: None,
        }
    }

    fn store(events: Vec<CurationEvent>) -> CurationStoreV1 {
        CurationStoreV1 {
            version: CURATION_STORE_VERSION,
            corpus_fingerprint: CorpusFingerprint("corpus".to_owned()),
            events,
        }
    }

    /// Serialize a store WITHOUT the domain validation `encode_store` enforces,
    /// so a decode test can feed bytes an honest writer would have refused.
    fn raw(store: &CurationStoreV1) -> Vec<u8> {
        serde_json::to_vec(store).expect("serializes")
    }

    #[test]
    fn v1_encode_decode_round_trips() {
        let s = store(vec![event("e1", "c1", "2026-07-18T10:00:00Z", "fp1")]);
        let decoded = decode_store(&encode_store(&s).expect("encodes")).expect("round trip");
        assert_eq!(decoded, s);
    }

    #[test]
    fn encode_is_independent_of_event_order() {
        let a = store(vec![
            event("e1", "c1", "2026-07-18T10:00:00Z", "fp1"),
            event("e2", "c2", "2026-07-18T11:00:00Z", "fp2"),
        ]);
        let mut b = a.clone();
        b.events.reverse();
        assert_eq!(
            encode_store(&a).expect("encodes"),
            encode_store(&b).expect("encodes")
        );
    }

    #[test]
    fn an_unknown_version_is_a_typed_refusal() {
        let bytes = br#"{"version":2,"corpus_fingerprint":"c","events":[]}"#;
        assert!(matches!(
            decode_store(bytes),
            Err(StoreDecodeError::Invalid(
                StoreValidationError::UnsupportedVersion { found: 2 }
            ))
        ));
    }

    #[test]
    fn malformed_json_is_not_an_empty_store() {
        // The named contract: a decode failure must never read as "no decisions".
        let err = decode_store(b"{ this is not json").expect_err("must fail");
        assert!(matches!(err, StoreDecodeError::MalformedStore(_)));
    }

    #[test]
    fn decode_refuses_a_duplicate_event_id() {
        let s = store(vec![
            event("dup", "c1", "2026-07-18T10:00:00Z", "fp1"),
            event("dup", "c2", "2026-07-18T11:00:00Z", "fp2"),
        ]);
        assert!(matches!(
            decode_store(&raw(&s)),
            Err(StoreDecodeError::Invalid(
                StoreValidationError::DuplicateEventId { .. }
            ))
        ));
    }

    #[test]
    fn decode_refuses_an_empty_opaque_id() {
        let s = store(vec![event("", "c1", "2026-07-18T10:00:00Z", "fp1")]);
        assert!(matches!(
            decode_store(&raw(&s)),
            Err(StoreDecodeError::Invalid(
                StoreValidationError::EmptyField { .. }
            ))
        ));
    }

    #[test]
    fn decode_refuses_an_empty_envelope_corpus_fingerprint() {
        let mut s = store(vec![event("e1", "c1", "2026-07-18T10:00:00Z", "fp1")]);
        s.corpus_fingerprint = CorpusFingerprint(String::new());
        assert!(matches!(
            decode_store(&raw(&s)),
            Err(StoreDecodeError::Invalid(
                StoreValidationError::EmptyCorpusFingerprint
            ))
        ));
    }

    #[test]
    fn encode_refuses_to_write_an_invalid_store() {
        // Every domain rule blocks a write, so a bad store never lands on disk
        // to be discovered only after a restart.
        let dup = store(vec![
            event("d", "c1", "2026-07-18T10:00:00Z", "fp1"),
            event("d", "c2", "2026-07-18T11:00:00Z", "fp2"),
        ]);
        assert!(matches!(
            encode_store(&dup),
            Err(StoreEncodeError::Invalid(
                StoreValidationError::DuplicateEventId { .. }
            ))
        ));

        let empty_id = store(vec![event("", "c1", "2026-07-18T10:00:00Z", "fp1")]);
        assert!(matches!(
            encode_store(&empty_id),
            Err(StoreEncodeError::Invalid(
                StoreValidationError::EmptyField { .. }
            ))
        ));

        let bad_time = store(vec![event("e1", "c1", "yesterday", "fp1")]);
        assert!(matches!(
            encode_store(&bad_time),
            Err(StoreEncodeError::Invalid(
                StoreValidationError::InvalidTimestamp { .. }
            ))
        ));

        let mut bad_version = store(vec![event("e1", "c1", "2026-07-18T10:00:00Z", "fp1")]);
        bad_version.version = 2;
        assert!(matches!(
            encode_store(&bad_version),
            Err(StoreEncodeError::Invalid(
                StoreValidationError::UnsupportedVersion { found: 2 }
            ))
        ));
    }

    #[test]
    fn a_refused_encode_never_yields_bytes() {
        // The failure mode we must not have: a serialize error swallowed into an
        // empty file that then reads back as "no decisions were ever made".
        let bad = store(vec![event("e1", "c1", "yesterday", "fp1")]);
        assert!(encode_store(&bad).is_err(), "no bytes for an invalid store");
    }

    #[test]
    fn an_unpinned_chunk_gets_a_typed_refusal() {
        // sha256, track_index, AND bar_range are all required for a durable
        // identity — a partial pin is no identity at all.
        assert!(matches!(
            chunk_fingerprint(&meta("c1", None, Some(0), Some((0, 3)))),
            Err(FingerprintError::UnpinnedChunk { .. })
        ));
        assert!(matches!(
            chunk_fingerprint(&meta("c1", Some("h"), None, Some((0, 3)))),
            Err(FingerprintError::UnpinnedChunk { .. })
        ));
        assert!(
            matches!(
                chunk_fingerprint(&meta("c1", Some("h"), Some(0), None)),
                Err(FingerprintError::UnpinnedChunk { .. })
            ),
            "a chunk with no bar_range is unpinned, not a whole-track identity"
        );
    }

    #[test]
    fn corpus_fingerprint_is_independent_of_chunk_order() {
        let a = [pinned("c1"), pinned("c2"), pinned("c3")];
        let mut b = a.clone();
        b.reverse();
        assert_eq!(
            corpus_fingerprint(9, &a).unwrap(),
            corpus_fingerprint(9, &b).unwrap()
        );
    }

    #[test]
    fn corpus_fingerprint_ignores_curator_mutable_fields() {
        let mut a = pinned("c1");
        let mut b = pinned("c1");
        a.tags = vec![SwancoreTag::PalmMute];
        a.reviewer = Some(ReviewerDecision::Accepted);
        a.updated_at = "2026-07-18T10:00:00Z".to_owned();
        b.tags = vec![SwancoreTag::Bend, SwancoreTag::Slide];
        b.reviewer = None;
        assert_eq!(
            corpus_fingerprint(9, &[a]).unwrap(),
            corpus_fingerprint(9, &[b]).unwrap()
        );
    }

    #[test]
    fn chunk_fingerprint_changes_with_sha_track_or_range() {
        let base = chunk_fingerprint(&pinned("c1")).unwrap();
        assert_ne!(
            base,
            chunk_fingerprint(&meta("c1", Some("other"), Some(0), Some((0, 3)))).unwrap()
        );
        assert_ne!(
            base,
            chunk_fingerprint(&meta("c1", Some("hashof_"), Some(1), Some((0, 3)))).unwrap()
        );
        assert_ne!(
            base,
            chunk_fingerprint(&meta("c1", Some("hashof_"), Some(0), Some((0, 7)))).unwrap()
        );
    }

    #[test]
    fn the_latest_decision_is_by_timestamp_then_event_id() {
        let mut early = event("e_z", "c1", "2026-07-18T10:00:00Z", "fp1");
        early.decision = ReviewerDecision::Rejected;
        let late = event("e_a", "c1", "2026-07-18T12:00:00Z", "fp1");
        let projected = project(&[late, early]);
        assert_eq!(
            projected.by_chunk[&ChunkId("c1".to_owned())].decision,
            ReviewerDecision::Accepted,
            "the later timestamp wins even with an alphabetically-earlier id"
        );
        // Same timestamp: the greater event_id breaks the tie.
        let a = event("e_a", "c2", "2026-07-18T10:00:00Z", "fp2");
        let mut z = event("e_z", "c2", "2026-07-18T10:00:00Z", "fp2");
        z.reviewer = ReviewerId("bob".to_owned());
        let tie = project(&[a, z]);
        assert_eq!(
            tie.by_chunk[&ChunkId("c2".to_owned())].reviewer,
            ReviewerId("bob".to_owned()),
            "the greater event_id wins on an equal timestamp"
        );
    }

    #[test]
    fn projection_is_independent_of_input_order() {
        let e1 = event("e1", "c1", "2026-07-18T10:00:00Z", "fp1");
        let e2 = event("e2", "c1", "2026-07-18T11:00:00Z", "fp1");
        assert_eq!(project(&[e1.clone(), e2.clone()]), project(&[e2, e1]));
    }

    #[test]
    fn corpus_expansion_keeps_an_old_decision_active() {
        let c1 = pinned("c1");
        let fp1 = chunk_fingerprint(&c1).unwrap();
        let s = store(vec![event("e1", "c1", "2026-07-18T10:00:00Z", &fp1.0)]);
        // A new chunk c2 joins the corpus; c1 is unchanged.
        let reconciled = reconcile(&s, &[c1, pinned("c2")]).unwrap();
        assert_eq!(reconciled.active.len(), 1, "c1's decision stays active");
        assert!(reconciled.orphaned.is_empty());
    }

    #[test]
    fn a_deleted_chunk_orphans_its_event() {
        let c1 = pinned("c1");
        let fp1 = chunk_fingerprint(&c1).unwrap();
        let s = store(vec![event("e1", "c1", "2026-07-18T10:00:00Z", &fp1.0)]);
        let reconciled = reconcile(&s, &[pinned("c2")]).unwrap();
        assert_eq!(reconciled.orphaned.len(), 1);
        assert_eq!(reconciled.orphaned[0].reason, OrphanReason::MissingChunk);
    }

    #[test]
    fn a_changed_chunk_orphans_its_event() {
        let s = store(vec![event("e1", "c1", "2026-07-18T10:00:00Z", "stale_fp")]);
        let reconciled = reconcile(&s, &[pinned("c1")]).unwrap();
        assert_eq!(reconciled.orphaned.len(), 1);
        assert_eq!(
            reconciled.orphaned[0].reason,
            OrphanReason::ChangedChunkFingerprint
        );
    }

    #[test]
    fn a_changed_cluster_context_does_not_orphan_a_decision() {
        let c1 = pinned("c1");
        let fp1 = chunk_fingerprint(&c1).unwrap();
        let mut e = event("e1", "c1", "2026-07-18T10:00:00Z", &fp1.0);
        // The cluster regrouped: a different representative and members.
        e.context.cluster_representative = ChunkId("other".to_owned());
        e.context.cluster_members = vec![ChunkId("c1".to_owned()), ChunkId("x".to_owned())];
        let reconciled = reconcile(&store(vec![e]), &[c1]).unwrap();
        assert_eq!(reconciled.active.len(), 1, "context is audit, not identity");
    }

    #[test]
    fn tags_are_canonicalized_deterministically() {
        let mut e = event("e1", "c1", "2026-07-18T10:00:00Z", "fp1");
        e.tags = vec![
            SwancoreTag::Slide,
            SwancoreTag::Bend,
            SwancoreTag::Slide, // duplicate
        ];
        let decoded =
            decode_store(&encode_store(&store(vec![e])).expect("encodes")).expect("decodes");
        // Canonical order is the tag enum's declaration order (Slide precedes
        // Bend), not alphabetical — deterministic is what matters, and the
        // duplicate is gone.
        assert_eq!(
            decoded.events[0].tags,
            vec![SwancoreTag::Slide, SwancoreTag::Bend],
            "sorted (by enum order) and deduped"
        );
    }

    #[test]
    fn decode_then_encode_is_one_canonical_byte_representation() {
        let mut s = store(vec![
            event("e2", "c2", "2026-07-18T11:00:00Z", "fp2"),
            event("e1", "c1", "2026-07-18T10:00:00Z", "fp1"),
        ]);
        s.events[0].tags = vec![SwancoreTag::Slide, SwancoreTag::Bend];
        let once = encode_store(&decode_store(&encode_store(&s).unwrap()).unwrap()).unwrap();
        let twice = encode_store(&decode_store(&once).unwrap()).unwrap();
        assert_eq!(once, twice, "decode->encode is idempotent bytes");
    }

    #[test]
    fn a_fractional_second_timestamp_is_rejected() {
        // Variable-width fractional seconds break lexicographic==chronological
        // ordering, so the canonical form is second precision only.
        for at in [
            "2026-07-18T10:00:00.500Z",
            "2026-07-18T10:00:00.1Z",
            "2026-07-18T10:00:00.100Z",
        ] {
            let s = store(vec![event("e1", "c1", at, "fp1")]);
            assert!(
                matches!(
                    encode_store(&s),
                    Err(StoreEncodeError::Invalid(
                        StoreValidationError::InvalidTimestamp { .. }
                    ))
                ),
                "fractional seconds rejected: {at}"
            );
        }
    }

    #[test]
    fn an_out_of_range_timestamp_field_is_rejected() {
        for at in [
            "2026-99-18T10:00:00Z", // month
            "2026-07-88T10:00:00Z", // day
            "2026-07-18T77:00:00Z", // hour
            "2026-07-18T10:66:00Z", // minute
            "2026-07-18T10:00:99Z", // second
            "2026-99-88T77:66:55Z", // all at once
        ] {
            let s = store(vec![event("e1", "c1", at, "fp1")]);
            assert!(
                matches!(
                    encode_store(&s),
                    Err(StoreEncodeError::Invalid(
                        StoreValidationError::InvalidTimestamp { .. }
                    ))
                ),
                "out-of-range field rejected: {at}"
            );
        }
    }

    #[test]
    fn an_impossible_calendar_day_is_rejected_but_a_leap_day_is_kept() {
        for bad in ["2026-02-30T00:00:00Z", "2025-02-29T00:00:00Z"] {
            let s = store(vec![event("e1", "c1", bad, "fp1")]);
            assert!(
                matches!(
                    encode_store(&s),
                    Err(StoreEncodeError::Invalid(
                        StoreValidationError::InvalidTimestamp { .. }
                    ))
                ),
                "impossible civil day rejected: {bad}"
            );
        }
        // A real leap day survives.
        let leap = store(vec![event("e1", "c1", "2024-02-29T00:00:00Z", "fp1")]);
        assert!(encode_store(&leap).is_ok(), "2024-02-29 is a real date");
    }

    #[test]
    fn projection_picks_the_chronologically_latest_event() {
        // The two timestamps sort the same way lexically and chronologically
        // only because the canonical form is fixed-width; this pins that the
        // later wall-clock decision is the projected one.
        let mut early = event("e_late_id", "c1", "2026-07-18T09:59:59Z", "fp1");
        early.decision = ReviewerDecision::Rejected;
        let late = event("e_early_id", "c1", "2026-07-18T10:00:00Z", "fp1");
        let projected = project(&[late, early]);
        assert_eq!(
            projected.by_chunk[&ChunkId("c1".to_owned())].decision,
            ReviewerDecision::Accepted,
            "the later second wins regardless of event_id ordering"
        );
    }
}
