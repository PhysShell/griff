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

use serde::{Deserialize, Serialize};

use crate::corpus::{source_sha256, ChunkId, ChunkMeta, ReviewerDecision, SwancoreTag};

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
    /// ISO-8601 UTC timestamp (validated on decode).
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

/// Why a store's bytes could not be decoded into a valid store.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreDecodeError {
    /// The bytes are not valid store JSON. Never treated as an empty store.
    MalformedStore(String),
    /// The `version` field is not one this module understands.
    UnsupportedVersion { found: u32 },
    /// Two events share an id.
    DuplicateEventId { id: CurationEventId },
    /// A required opaque field was empty.
    EmptyField {
        field: &'static str,
        event: CurationEventId,
    },
    /// `occurred_at` is not a valid ISO-8601 UTC timestamp.
    InvalidTimestamp {
        event: CurationEventId,
        value: String,
    },
}

/// Why a fingerprint could not be computed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FingerprintError {
    /// The chunk lacks the pinned identity (`sha256` / `track_index`) a durable
    /// decision must bind to; a filename is not an identity.
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
#[derive(Debug, Clone, PartialEq, Eq)]
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

/// The pinned material fingerprint of one chunk: `ChunkId` + `sha256` +
/// `track_index` + `bar_range`, hashed. A chunk without `sha256` or
/// `track_index` is [`FingerprintError::UnpinnedChunk`] — no filename fallback.
#[allow(clippy::missing_errors_doc)]
pub fn chunk_fingerprint(_chunk: &ChunkMeta) -> Result<ChunkFingerprint, FingerprintError> {
    Ok(ChunkFingerprint("stub".to_owned()))
}

/// The corpus fingerprint: the schema version plus the sorted set of chunk
/// fingerprints, hashed. Independent of chunk order and of any curator-mutable
/// field (tags, reviewer, timestamps).
#[allow(clippy::missing_errors_doc)]
pub fn corpus_fingerprint(
    _schema_version: u32,
    _chunks: &[ChunkMeta],
) -> Result<CorpusFingerprint, FingerprintError> {
    Ok(CorpusFingerprint("stub".to_owned()))
}

// ── encode / decode ─────────────────────────────────────────────────────────

/// Serializes a store into its one canonical byte form (events and tags
/// canonicalized first), so the same logical store always encodes identically.
#[must_use]
pub fn encode_store(_store: &CurationStoreV1) -> Vec<u8> {
    Vec::new()
}

/// Decodes and validates store bytes, canonicalizing the result. A malformed
/// store is a typed error, never an empty store.
#[allow(clippy::missing_errors_doc)]
pub fn decode_store(_bytes: &[u8]) -> Result<CurationStoreV1, StoreDecodeError> {
    Err(StoreDecodeError::MalformedStore("stub".to_owned()))
}

// ── projection ──────────────────────────────────────────────────────────────

/// Projects events to the latest decision per chunk, by `(occurred_at,
/// event_id)`. Independent of input order; cluster context is not projected.
#[must_use]
pub fn project(_events: &[CurationEvent]) -> ProjectedCuration {
    ProjectedCuration {
        by_chunk: BTreeMap::new(),
    }
}

// ── reconciliation ──────────────────────────────────────────────────────────

/// Reconciles a store's events against the current corpus. An event stays
/// active when its chunk is present with the same pinned fingerprint (even if
/// the shared corpus fingerprint moved, or the cluster regrouped); it is
/// orphaned when the chunk is gone or its material changed. No fuzzy remap.
#[allow(clippy::missing_errors_doc)]
pub fn reconcile(
    _store: &CurationStoreV1,
    _current_chunks: &[ChunkMeta],
) -> Result<ReconciledCuration, FingerprintError> {
    Ok(ReconciledCuration {
        active: Vec::new(),
        orphaned: Vec::new(),
        corpus_match: false,
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::unwrap_used)]

    use super::{
        chunk_fingerprint, corpus_fingerprint, decode_store, encode_store, project, reconcile,
        ChunkFingerprint, CorpusFingerprint, CurationContext, CurationEvent, CurationEventId,
        CurationStoreV1, FingerprintError, OrphanReason, ReviewerId, StoreDecodeError,
        CURATION_STORE_VERSION,
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
        meta(id, Some("hashof_".into()), Some(0), Some((0, 3)))
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

    #[test]
    fn v1_encode_decode_round_trips() {
        let s = store(vec![event("e1", "c1", "2026-07-18T10:00:00Z", "fp1")]);
        let decoded = decode_store(&encode_store(&s)).expect("round trip");
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
        assert_eq!(encode_store(&a), encode_store(&b));
    }

    #[test]
    fn an_unknown_version_is_a_typed_refusal() {
        let bytes = br#"{"version":2,"corpus_fingerprint":"c","events":[]}"#;
        assert!(matches!(
            decode_store(bytes),
            Err(StoreDecodeError::UnsupportedVersion { found: 2 })
        ));
    }

    #[test]
    fn malformed_json_is_not_an_empty_store() {
        // The named contract: a decode failure must never read as "no decisions".
        let err = decode_store(b"{ this is not json").expect_err("must fail");
        assert!(matches!(err, StoreDecodeError::MalformedStore(_)));
    }

    #[test]
    fn a_duplicate_event_id_is_rejected() {
        let s = store(vec![
            event("dup", "c1", "2026-07-18T10:00:00Z", "fp1"),
            event("dup", "c2", "2026-07-18T11:00:00Z", "fp2"),
        ]);
        assert!(matches!(
            decode_store(&encode_store(&s)),
            Err(StoreDecodeError::DuplicateEventId { .. })
        ));
    }

    #[test]
    fn an_empty_opaque_id_is_rejected() {
        let s = store(vec![event("", "c1", "2026-07-18T10:00:00Z", "fp1")]);
        assert!(matches!(
            decode_store(&encode_store(&s)),
            Err(StoreDecodeError::EmptyField { .. })
        ));
    }

    #[test]
    fn an_invalid_timestamp_is_rejected() {
        let s = store(vec![event("e1", "c1", "yesterday", "fp1")]);
        assert!(matches!(
            decode_store(&encode_store(&s)),
            Err(StoreDecodeError::InvalidTimestamp { .. })
        ));
    }

    #[test]
    fn an_unpinned_chunk_gets_a_typed_refusal() {
        assert!(matches!(
            chunk_fingerprint(&meta("c1", None, Some(0), Some((0, 3)))),
            Err(FingerprintError::UnpinnedChunk { .. })
        ));
        assert!(matches!(
            chunk_fingerprint(&meta("c1", Some("h"), None, Some((0, 3)))),
            Err(FingerprintError::UnpinnedChunk { .. })
        ));
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
        let projected = project(&[late.clone(), early]);
        assert_eq!(
            projected.by_chunk[&ChunkId("c1".to_owned())].decision,
            ReviewerDecision::Accepted,
            "the later timestamp wins even with an alphabetically-earlier id"
        );
        // Same timestamp: the greater event_id breaks the tie.
        let a = event("e_a", "c2", "2026-07-18T10:00:00Z", "fp2");
        let mut z = event("e_z", "c2", "2026-07-18T10:00:00Z", "fp2");
        z.reviewer = ReviewerId("bob".to_owned());
        let projected = project(&[a, z]);
        assert_eq!(
            projected.by_chunk[&ChunkId("c2".to_owned())].reviewer,
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
        let decoded = decode_store(&encode_store(&store(vec![e]))).expect("decodes");
        assert_eq!(
            decoded.events[0].tags,
            vec![SwancoreTag::Bend, SwancoreTag::Slide],
            "sorted and deduped"
        );
    }

    #[test]
    fn decode_then_encode_is_one_canonical_byte_representation() {
        let mut s = store(vec![
            event("e2", "c2", "2026-07-18T11:00:00Z", "fp2"),
            event("e1", "c1", "2026-07-18T10:00:00Z", "fp1"),
        ]);
        s.events[0].tags = vec![SwancoreTag::Slide, SwancoreTag::Bend];
        let once = encode_store(&decode_store(&encode_store(&s)).unwrap());
        let twice = encode_store(&decode_store(&once).unwrap());
        assert_eq!(once, twice, "decode->encode is idempotent bytes");
    }
}
