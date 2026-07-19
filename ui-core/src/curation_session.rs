//! In-session curation state and boundary event minting (step 3C-B1).
//!
//! A pure, renderer-agnostic layer over the merged [`griff_core::curation_store`]
//! domain model: it holds the append-only store for one editing session, assembles
//! a [`CurationEvent`] from a decision plus the corpus (fingerprints and the
//! cluster audit snapshot), and projects/reconciles through core. **No I/O and no
//! UI** — a frontend supplies the opaque `event_id` and canonical `occurred_at`
//! (minted here from a boundary-provided random seed and clock), drives
//! [`CurationSession::record`], then persists [`CurationSession::store`] through
//! the OPFS/native adapter.
//!
//! A decision is always **appended**, keyed by [`ChunkId`]; nothing is mutated in
//! place. Cluster context is an audit snapshot on the event, never part of the
//! decision's identity (ADR-0030 §8): a regrouped cluster does not move a
//! decision. That invariant lives in core's `reconcile`; this layer only feeds it.

use std::fmt::Write as _;

use griff_core::corpus::{ChunkId, ChunkMeta, ReviewerDecision, SwancoreTag, SCHEMA_VERSION};
use griff_core::curation::build_clusters;
use griff_core::curation_store::{
    chunk_fingerprint, corpus_fingerprint, project, reconcile as core_reconcile, CurationContext,
    CurationEvent, CurationEventId, CurationStoreV1, FingerprintError, ProjectedCuration,
    ReconciledCuration, ReviewerId, CURATION_STORE_VERSION,
};

/// One decision to record.
///
/// The opaque `event_id` and canonical `occurred_at` come from the boundary
/// (mint with [`mint_event_id`] / [`canonical_timestamp`]); the fingerprints and
/// cluster context are derived from the corpus by [`CurationSession::record`].
#[derive(Debug, Clone)]
pub struct RecordRequest {
    pub chunk_id: ChunkId,
    pub decision: ReviewerDecision,
    pub reviewer: ReviewerId,
    pub event_id: CurationEventId,
    pub occurred_at: String,
    pub tags: Vec<SwancoreTag>,
    pub note: Option<String>,
}

/// Why a decision could not be recorded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RecordError {
    /// The decision names a chunk that is not in the current corpus.
    UnknownChunk { chunk_id: ChunkId },
    /// The chunk exists but lacks a pinned identity to bind the decision to.
    Unpinned(FingerprintError),
}

/// The append-only curation store for one editing session.
#[derive(Debug, Clone)]
pub struct CurationSession {
    store: CurationStoreV1,
}

impl CurationSession {
    /// Bootstraps from a loaded store, or a fresh **empty** store pinned to the
    /// current corpus when there is none yet — a typed empty bootstrap, never a
    /// guess.
    ///
    /// # Errors
    /// [`FingerprintError`] if the corpus cannot be fingerprinted (an unpinned
    /// chunk).
    pub fn bootstrap(
        loaded: Option<CurationStoreV1>,
        corpus: &[ChunkMeta],
    ) -> Result<Self, FingerprintError> {
        let store = match loaded {
            Some(store) => store,
            None => CurationStoreV1 {
                version: CURATION_STORE_VERSION,
                corpus_fingerprint: corpus_fingerprint(SCHEMA_VERSION, corpus)?,
                events: Vec::new(),
            },
        };
        Ok(Self { store })
    }

    /// Records a decision by appending a new [`CurationEvent`] — never mutating an
    /// existing one.
    ///
    /// # Errors
    /// [`RecordError::UnknownChunk`] if the chunk is absent from `corpus`;
    /// [`RecordError::Unpinned`] if it lacks a pinned identity.
    pub fn record(&mut self, req: RecordRequest, corpus: &[ChunkMeta]) -> Result<(), RecordError> {
        let chunk = corpus
            .iter()
            .find(|c| c.id == req.chunk_id)
            .ok_or_else(|| RecordError::UnknownChunk {
                chunk_id: req.chunk_id.clone(),
            })?;
        let chunk_fingerprint = chunk_fingerprint(chunk).map_err(RecordError::Unpinned)?;
        let corpus_fingerprint =
            corpus_fingerprint(SCHEMA_VERSION, corpus).map_err(RecordError::Unpinned)?;
        let context = cluster_context(&req.chunk_id, corpus);
        self.store.events.push(CurationEvent {
            event_id: req.event_id,
            chunk_id: req.chunk_id,
            chunk_fingerprint,
            decision: req.decision,
            reviewer: req.reviewer,
            occurred_at: req.occurred_at,
            corpus_fingerprint,
            context,
            tags: req.tags,
            note: req.note,
        });
        Ok(())
    }

    /// The latest decision per chunk (core's projection; cluster context excluded).
    #[must_use]
    pub fn projected(&self) -> ProjectedCuration {
        project(&self.store.events)
    }

    /// Reconciles the session's decisions against the current corpus.
    ///
    /// # Errors
    /// [`FingerprintError`] if a current chunk cannot be fingerprinted.
    pub fn reconcile(&self, corpus: &[ChunkMeta]) -> Result<ReconciledCuration, FingerprintError> {
        core_reconcile(&self.store, corpus)
    }

    /// The store, for the boundary to encode and persist.
    #[must_use]
    pub const fn store(&self) -> &CurationStoreV1 {
        &self.store
    }
}

/// The cluster audit snapshot for `chunk_id`.
///
/// Its dedup cluster's representative and all members (itself when it is a
/// singleton). Never part of the decision's identity — a navigation/audit fact.
fn cluster_context(chunk_id: &ChunkId, corpus: &[ChunkMeta]) -> CurationContext {
    let (clusters, _diagnostics) = build_clusters(corpus);
    for cluster in &clusters {
        if &cluster.representative == chunk_id || cluster.variants.contains(chunk_id) {
            let mut members = vec![cluster.representative.clone()];
            members.extend(cluster.variants.iter().cloned());
            members.sort();
            return CurationContext {
                cluster_representative: cluster.representative.clone(),
                cluster_members: members,
            };
        }
    }
    CurationContext {
        cluster_representative: chunk_id.clone(),
        cluster_members: vec![chunk_id.clone()],
    }
}

/// Mints an opaque decision-event id: an RFC-4122 **v4 UUID**.
///
/// From 16 boundary-supplied random bytes — the randomness is the boundary's;
/// this only stamps the version (4) and variant (10) bits and the canonical
/// hyphenated hex layout, so the id is opaque, unique, and stable.
#[must_use]
pub fn mint_event_id(bytes: [u8; 16]) -> CurationEventId {
    let id = bytes
        .iter()
        .enumerate()
        .map(|(i, &b)| match i {
            6 => (b & 0x0f) | 0x40, // version 4
            8 => (b & 0x3f) | 0x80, // variant 10xx
            _ => b,
        })
        .enumerate()
        .fold(String::with_capacity(36), |mut acc, (i, byte)| {
            if matches!(i, 4 | 6 | 8 | 10) {
                acc.push('-');
            }
            // Writing hex into a String is infallible.
            write!(acc, "{byte:02x}").ok();
            acc
        });
    CurationEventId(id)
}

/// Formats a Unix-epoch second count as the canonical timestamp the store wants.
///
/// `YYYY-MM-DDTHH:MM:SSZ` — second precision, no fractional part (which
/// `curation_store` rejects). Howard Hinnant's civil-from-days, no `chrono`.
#[must_use]
// Bounded civil-from-days: `epoch_secs` is clamped non-negative and every
// intermediate product stays well within i64 for any realistic year, so the
// arithmetic cannot overflow — the lint carries no signal here.
#[allow(clippy::arithmetic_side_effects)]
pub fn canonical_timestamp(epoch_secs: i64) -> String {
    let secs = epoch_secs.max(0);
    let (hh, mm, ss) = (secs / 3600 % 24, secs / 60 % 60, secs % 60);
    let z = secs / 86_400 + 719_468;
    let era = (if z >= 0 { z } else { z - 146_096 }) / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}T{hh:02}:{mm:02}:{ss:02}Z")
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::unwrap_used,
        clippy::panic,
        clippy::indexing_slicing
    )]

    use super::{canonical_timestamp, mint_event_id, CurationSession, RecordError, RecordRequest};
    use griff_core::corpus::{
        ChunkId, ChunkMeta, ReviewerDecision, SourceFormat, SourceRef, SCHEMA_VERSION,
    };
    use griff_core::curation_store::{
        corpus_fingerprint, decode_store, encode_store, CurationEventId, ProjectedDecision,
        ReviewerId,
    };
    use griff_core::novelty::PhraseDuplicate;

    fn chunk(id: &str, sha: &str, dup: Option<usize>) -> ChunkMeta {
        ChunkMeta {
            id: ChunkId(id.to_owned()),
            title: String::new(),
            source: SourceRef {
                filename: "s.gp".to_owned(),
                format: SourceFormat::Gp,
                bar_range: Some((0, 3)),
                track_index: Some(0),
                sha256: Some(sha.to_owned()),
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
            duplicate: dup.map(|of| PhraseDuplicate {
                of,
                quote_share: 0.9,
            }),
            style_cohort: None,
            ensemble: None,
            rights: None,
            created_at: String::new(),
            updated_at: String::new(),
        }
    }

    fn corpus() -> Vec<ChunkMeta> {
        vec![
            chunk("a_p0", "hash0", None),
            chunk("a_p1", "hash1", Some(0)), // a_p1 duplicates a_p0 → cluster {a_p0,[a_p1]}
            chunk("a_p2", "hash2", None),
        ]
    }

    fn request(chunk: &str, event: &str, at: &str, decision: ReviewerDecision) -> RecordRequest {
        RecordRequest {
            chunk_id: ChunkId(chunk.to_owned()),
            decision,
            reviewer: ReviewerId("alice".to_owned()),
            event_id: CurationEventId(event.to_owned()),
            occurred_at: at.to_owned(),
            tags: Vec::new(),
            note: None,
        }
    }

    fn decision(p: &ProjectedDecision) -> ReviewerDecision {
        p.decision
    }

    #[test]
    fn bootstrap_missing_pins_an_empty_store_to_the_corpus() {
        let corpus = corpus();
        let session = CurationSession::bootstrap(None, &corpus).expect("bootstrap");
        assert!(session.store().events.is_empty(), "no decisions yet");
        assert_eq!(
            session.store().corpus_fingerprint,
            corpus_fingerprint(SCHEMA_VERSION, &corpus).expect("fp"),
            "the empty store is pinned to the current corpus"
        );
        // A pinned empty store is itself valid and encodable.
        encode_store(session.store()).expect("empty store encodes");
    }

    #[test]
    fn record_appends_a_new_event_and_projects_it() {
        let corpus = corpus();
        let mut session = CurationSession::bootstrap(None, &corpus).expect("bootstrap");
        session
            .record(
                request(
                    "a_p0",
                    "e1",
                    "2026-07-19T10:00:00Z",
                    ReviewerDecision::Accepted,
                ),
                &corpus,
            )
            .expect("record");
        assert_eq!(session.store().events.len(), 1);
        let projected = session.projected();
        assert_eq!(
            decision(&projected.by_chunk[&ChunkId("a_p0".to_owned())]),
            ReviewerDecision::Accepted,
        );
    }

    #[test]
    fn a_repeat_decision_appends_a_new_event_never_mutates() {
        let corpus = corpus();
        let mut session = CurationSession::bootstrap(None, &corpus).expect("bootstrap");
        session
            .record(
                request(
                    "a_p0",
                    "e1",
                    "2026-07-19T10:00:00Z",
                    ReviewerDecision::Rejected,
                ),
                &corpus,
            )
            .expect("record 1");
        session
            .record(
                request(
                    "a_p0",
                    "e2",
                    "2026-07-19T11:00:00Z",
                    ReviewerDecision::Accepted,
                ),
                &corpus,
            )
            .expect("record 2");
        assert_eq!(
            session.store().events.len(),
            2,
            "two events, not a mutation"
        );
        assert_eq!(
            decision(&session.projected().by_chunk[&ChunkId("a_p0".to_owned())]),
            ReviewerDecision::Accepted,
            "the later decision wins",
        );
    }

    #[test]
    fn an_equal_timestamp_breaks_by_event_id() {
        let corpus = corpus();
        let mut session = CurationSession::bootstrap(None, &corpus).expect("bootstrap");
        session
            .record(
                request(
                    "a_p0",
                    "e_a",
                    "2026-07-19T10:00:00Z",
                    ReviewerDecision::Rejected,
                ),
                &corpus,
            )
            .expect("record a");
        session
            .record(
                request(
                    "a_p0",
                    "e_z",
                    "2026-07-19T10:00:00Z",
                    ReviewerDecision::Accepted,
                ),
                &corpus,
            )
            .expect("record z");
        assert_eq!(
            decision(&session.projected().by_chunk[&ChunkId("a_p0".to_owned())]),
            ReviewerDecision::Accepted,
            "the greater event_id wins on an equal timestamp",
        );
    }

    #[test]
    fn record_snapshots_the_cluster_context() {
        let corpus = corpus();
        let mut session = CurationSession::bootstrap(None, &corpus).expect("bootstrap");
        session
            .record(
                request(
                    "a_p1",
                    "e1",
                    "2026-07-19T10:00:00Z",
                    ReviewerDecision::Accepted,
                ),
                &corpus,
            )
            .expect("record");
        let context = &session.store().events[0].context;
        assert_eq!(
            context.cluster_members,
            vec![ChunkId("a_p0".to_owned()), ChunkId("a_p1".to_owned())],
            "the dedup cluster {{c0, c1}} is snapshotted as audit context",
        );
    }

    #[test]
    fn a_regrouped_cluster_does_not_move_a_decision() {
        let corpus = corpus();
        let mut session = CurationSession::bootstrap(None, &corpus).expect("bootstrap");
        session
            .record(
                request(
                    "a_p1",
                    "e1",
                    "2026-07-19T10:00:00Z",
                    ReviewerDecision::Accepted,
                ),
                &corpus,
            )
            .expect("record");
        // The corpus regroups: a_p1 no longer duplicates a_p0 (its cluster
        // changed), but its own material (sha/track/range) is unchanged.
        let regrouped = vec![
            chunk_no_dup("a_p0", "hash0"),
            chunk_no_dup("a_p1", "hash1"),
            chunk_no_dup("a_p2", "hash2"),
        ];
        let reconciled = session.reconcile(&regrouped).expect("reconcile");
        assert_eq!(reconciled.active.len(), 1, "the decision stays active");
        assert!(reconciled.orphaned.is_empty());
    }

    #[test]
    fn a_deleted_or_changed_chunk_is_orphaned() {
        let corpus = corpus();
        let mut session = CurationSession::bootstrap(None, &corpus).expect("bootstrap");
        session
            .record(
                request(
                    "a_p0",
                    "e1",
                    "2026-07-19T10:00:00Z",
                    ReviewerDecision::Accepted,
                ),
                &corpus,
            )
            .expect("record");
        // a_p0's material changes (new sha); a_p1/a_p2 gone.
        let changed = vec![chunk_no_dup("a_p0", "DIFFERENT")];
        let reconciled = session.reconcile(&changed).expect("reconcile");
        assert_eq!(reconciled.orphaned.len(), 1, "the changed chunk orphans it");
        assert!(reconciled.active.is_empty());
    }

    #[test]
    fn record_rejects_an_unknown_chunk() {
        let corpus = corpus();
        let mut session = CurationSession::bootstrap(None, &corpus).expect("bootstrap");
        let err = session
            .record(
                request(
                    "ghost",
                    "e1",
                    "2026-07-19T10:00:00Z",
                    ReviewerDecision::Accepted,
                ),
                &corpus,
            )
            .expect_err("unknown chunk must fail");
        assert!(matches!(err, RecordError::UnknownChunk { .. }));
    }

    #[test]
    fn mint_event_id_is_a_canonical_v4_uuid() {
        let id = mint_event_id([0xab; 16]).0;
        assert_eq!(id.len(), 36, "8-4-4-4-12 hex with hyphens");
        let dashes: Vec<usize> = id.match_indices('-').map(|(i, _)| i).collect();
        assert_eq!(
            dashes,
            vec![8, 13, 18, 23],
            "hyphens in the canonical spots"
        );
        assert_eq!(id.as_bytes()[14], b'4', "version nibble is 4");
        assert!(
            matches!(id.as_bytes()[19], b'8' | b'9' | b'a' | b'b'),
            "variant nibble is 8..b",
        );
        // Different seeds mint different ids.
        assert_ne!(mint_event_id([1; 16]).0, mint_event_id([2; 16]).0);
    }

    #[test]
    fn canonical_timestamp_is_second_precision_utc() {
        // Two well-known anchors, so the assertion doesn't depend on my own
        // epoch arithmetic: the epoch itself, and "one billion seconds".
        assert_eq!(canonical_timestamp(0), "1970-01-01T00:00:00Z");
        assert_eq!(
            canonical_timestamp(1_000_000_000),
            "2001-09-09T01:46:40Z",
            "exact civil time, second precision, no fractional part",
        );
        // A minted timestamp is accepted by the store (it round-trips through decode).
        let mut session = CurationSession::bootstrap(None, &corpus()).expect("bootstrap");
        session
            .record(
                request(
                    "a_p0",
                    "e1",
                    &canonical_timestamp(1_000_000_000),
                    ReviewerDecision::Accepted,
                ),
                &corpus(),
            )
            .expect("record");
        decode_store(&encode_store(session.store()).expect("encode")).expect("minted ts is valid");
    }

    /// A pinned chunk with no duplicate link.
    fn chunk_no_dup(id: &str, sha: &str) -> ChunkMeta {
        chunk(id, sha, None)
    }
}
