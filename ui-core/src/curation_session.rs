//! In-session curation state and boundary event minting (step 3C-B1).
//!
//! A pure, renderer-agnostic layer over the merged [`griff_core::curation_store`]
//! domain model: it holds the append-only store for one editing session, assembles
//! a [`CurationEvent`] from a decision plus the corpus (fingerprints and the
//! cluster audit snapshot), and projects/reconciles through core. **No I/O and no
//! UI** — a frontend mints the `event_id` and canonical `occurred_at` (see
//! [`EventIdMinter`] / [`canonical_timestamp`]) from a boundary clock and random
//! seed, drives [`CurationSession::record`], then persists
//! [`CurationSession::store`] through the OPFS/native adapter.
//!
//! A decision is always **appended**, keyed by [`ChunkId`]; nothing is mutated in
//! place. Cluster context is an audit snapshot on the event, never part of the
//! decision's identity: a regrouped cluster does not move a decision — the
//! invariant `curation_store::reconcile` enforces and this layer only feeds.

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
/// (mint with [`EventIdMinter`] / [`canonical_timestamp`]); the fingerprints and
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

/// A stateful, **order-preserving** minter for decision-event ids.
///
/// The store's projection picks the latest decision per chunk by `(occurred_at,
/// event_id)`, so within one canonical second the `event_id` must decide — and a
/// bare random UUID does not: two clicks in the same second could invert. Each
/// minted id therefore leads with a monotonic ordering prefix,
/// `web-{ms:016x}-{seq:08x}-{uuid}`, and the UUID is only the uniqueness tail:
///
/// - a later mint in the same second sorts **above** the previous one;
/// - after a reload, [`EventIdMinter::from_store`] seeds the prefix above every
///   id already saved, so the first new event outranks them;
/// - the prefix is a Lamport clock (`max(now, high-water)`), so a **clock
///   rollback never lowers it**;
/// - the tail keeps every id unique.
// Deliberately not `Copy`: it is a mutable accumulator, and a silent copy would
// fork the high-water counter and let two "minters" issue colliding orders.
#[allow(missing_copy_implementations)]
#[derive(Debug, Clone)]
pub struct EventIdMinter {
    /// The highest millisecond issued so far — never decreases.
    high_water_ms: u64,
    /// The sub-counter within `high_water_ms`, so equal/rolled-back clocks still
    /// advance.
    seq: u32,
}

impl EventIdMinter {
    /// Seeds the minter above every id already in `store`, so the next mint sorts
    /// above all saved decisions regardless of their order in the store.
    #[must_use]
    pub fn from_store(store: &CurationStoreV1) -> Self {
        // The seed is a max over every parseable id — order-independent.
        let (high_water_ms, seq) = store
            .events
            .iter()
            .filter_map(|event| parse_order(&event.event_id.0))
            .max()
            .unwrap_or((0, 0));
        Self { high_water_ms, seq }
    }

    /// Mints the next ordered, unique id from the boundary clock (`now_ms`) and a
    /// random seed.
    pub fn mint(&mut self, now_ms: u64, random: [u8; 16]) -> CurationEventId {
        // Lamport clock: advance the millisecond when it moves forward, otherwise
        // bump the sub-counter, so an equal or rolled-back clock still increases.
        if now_ms > self.high_water_ms {
            self.high_water_ms = now_ms;
            self.seq = 0;
        } else {
            self.seq = self.seq.saturating_add(1);
        }
        let uuid = uuid_v4(random);
        CurationEventId(format!(
            "web-{:016x}-{:08x}-{uuid}",
            self.high_water_ms, self.seq
        ))
    }
}

/// Parses the `(ms, seq)` ordering prefix of a `web-{ms:016x}-{seq:08x}-…` id.
/// `None` for any id not in that form — it simply does not seed the high-water.
fn parse_order(id: &str) -> Option<(u64, u32)> {
    let rest = id.strip_prefix("web-")?;
    let (ms_hex, rest) = rest.split_once('-')?;
    let (seq_hex, _) = rest.split_once('-')?;
    let ms = u64::from_str_radix(ms_hex, 16).ok()?;
    let seq = u32::from_str_radix(seq_hex, 16).ok()?;
    Some((ms, seq))
}

/// Formats 16 random bytes as an RFC-4122 **v4 UUID** — the uniqueness tail of a
/// minted id. Stamps the version (4) and variant (10) bits and the canonical
/// hyphenated hex layout.
fn uuid_v4(bytes: [u8; 16]) -> String {
    bytes
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
        })
}

/// Why a clock reading could not become a canonical timestamp.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimestampError {
    /// The clock reported a time before the Unix epoch. A failing or misconfigured
    /// clock is a typed refusal, never a normalised `1970-01-01T00:00:00Z`.
    BeforeEpoch { epoch_secs: i64 },
    /// The time falls outside the representable four-digit civil-year range.
    OutOfRange { epoch_secs: i64 },
}

/// Formats a Unix-epoch second count as the canonical timestamp the store wants.
///
/// `YYYY-MM-DDTHH:MM:SSZ` — second precision, no fractional part (which
/// `curation_store` rejects). Howard Hinnant's civil-from-days, no `chrono`.
///
/// # Errors
/// [`TimestampError::BeforeEpoch`] for a negative time; [`TimestampError::OutOfRange`]
/// for a year outside `0000..=9999`. A bad clock is refused, not silently made 1970.
// Bounded civil-from-days: `epoch_secs` is non-negative here and every
// intermediate product stays well within i64 for any representable year, so the
// arithmetic cannot overflow — the lint carries no signal.
#[allow(clippy::arithmetic_side_effects)]
pub fn canonical_timestamp(epoch_secs: i64) -> Result<String, TimestampError> {
    if epoch_secs < 0 {
        return Err(TimestampError::BeforeEpoch { epoch_secs });
    }
    let secs = epoch_secs;
    let (hh, mm, ss) = (secs / 3600 % 24, secs / 60 % 60, secs % 60);
    let z = secs / 86_400 + 719_468;
    let era = z / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    if !(0..=9999).contains(&year) {
        return Err(TimestampError::OutOfRange { epoch_secs });
    }
    Ok(format!(
        "{year:04}-{month:02}-{day:02}T{hh:02}:{mm:02}:{ss:02}Z"
    ))
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::unwrap_used,
        clippy::panic,
        clippy::indexing_slicing
    )]

    use super::{
        canonical_timestamp, CurationSession, EventIdMinter, RecordError, RecordRequest,
        TimestampError,
    };
    use griff_core::corpus::{
        ChunkId, ChunkMeta, ReviewerDecision, SourceFormat, SourceRef, SCHEMA_VERSION,
    };
    use griff_core::curation_store::{
        corpus_fingerprint, decode_store, encode_store, ChunkFingerprint, CorpusFingerprint,
        CurationContext, CurationEvent, CurationEventId, CurationStoreV1, ProjectedDecision,
        ReviewerId, CURATION_STORE_VERSION,
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
    fn a_later_mint_in_the_same_second_sorts_higher() {
        let mut minter = EventIdMinter::from_store(&empty_store());
        // The second seed's UUID sorts *below* the first's, so a random-only id
        // would invert — the ordering prefix must win.
        let first = minter.mint(1_000, [0xff; 16]).0;
        let second = minter.mint(1_000, [0x00; 16]).0;
        assert!(
            second > first,
            "the later click sorts higher: {second} > {first}"
        );
    }

    #[test]
    fn a_reload_mints_above_the_highest_saved_id() {
        let saved = oid(0x1_0000, 5);
        let mut minter = EventIdMinter::from_store(&store_with_ids(&[&saved]));
        // Even with a tiny clock, the reloaded minter continues above the saved id.
        let next = minter.mint(1, [0; 16]).0;
        assert!(
            next > saved,
            "the reloaded minter outranks the saved id: {next} > {saved}"
        );
    }

    #[test]
    fn a_clock_rollback_does_not_lower_the_id() {
        let mut minter = EventIdMinter::from_store(&empty_store());
        let first = minter.mint(1_000, [0; 16]).0;
        // The clock jumps backwards; the next id must still sort higher.
        let second = minter.mint(500, [0; 16]).0;
        assert!(
            second > first,
            "a rolled-back clock never lowers the id: {second} > {first}"
        );
    }

    #[test]
    fn mints_are_unique() {
        let mut minter = EventIdMinter::from_store(&empty_store());
        let ids: Vec<String> = (0..8).map(|i| minter.mint(1_000, [i; 16]).0).collect();
        let mut sorted = ids.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), ids.len(), "every minted id is unique");
    }

    #[test]
    fn from_store_is_independent_of_event_order() {
        let forward = store_with_ids(&[&oid(1, 1), &oid(2, 2)]);
        let reversed = store_with_ids(&[&oid(2, 2), &oid(1, 1)]);
        let a = EventIdMinter::from_store(&forward).mint(0, [0; 16]).0;
        let b = EventIdMinter::from_store(&reversed).mint(0, [0; 16]).0;
        assert_eq!(a, b, "the seed is a max over events, not order-dependent");
    }

    #[test]
    fn canonical_timestamp_is_second_precision_utc() {
        // Two well-known anchors, so the assertion doesn't depend on my own
        // epoch arithmetic: the epoch itself, and "one billion seconds".
        assert_eq!(
            canonical_timestamp(0).expect("epoch"),
            "1970-01-01T00:00:00Z"
        );
        assert_eq!(
            canonical_timestamp(1_000_000_000).expect("1e9"),
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
                    &canonical_timestamp(1_000_000_000).expect("1e9"),
                    ReviewerDecision::Accepted,
                ),
                &corpus(),
            )
            .expect("record");
        decode_store(&encode_store(session.store()).expect("encode")).expect("minted ts is valid");
    }

    #[test]
    fn a_bad_clock_is_a_typed_error_not_a_1970_lie() {
        assert_eq!(
            canonical_timestamp(-1),
            Err(TimestampError::BeforeEpoch { epoch_secs: -1 }),
            "a negative clock is refused, never normalised to the epoch",
        );
        // A time past the four-digit-year range is also refused.
        assert!(matches!(
            canonical_timestamp(i64::MAX),
            Err(TimestampError::OutOfRange { .. })
        ));
    }

    /// A pinned chunk with no duplicate link.
    fn chunk_no_dup(id: &str, sha: &str) -> ChunkMeta {
        chunk(id, sha, None)
    }

    /// A web-format ordering id with the given prefix and a fixed UUID tail.
    fn oid(ms: u64, seq: u32) -> String {
        format!("web-{ms:016x}-{seq:08x}-00000000-0000-4000-8000-000000000000")
    }

    fn empty_store() -> CurationStoreV1 {
        store_with_ids(&[])
    }

    fn store_with_ids(ids: &[&str]) -> CurationStoreV1 {
        CurationStoreV1 {
            version: CURATION_STORE_VERSION,
            corpus_fingerprint: CorpusFingerprint("corpus".to_owned()),
            events: ids.iter().map(|id| event_with_id(id)).collect(),
        }
    }

    fn event_with_id(id: &str) -> CurationEvent {
        CurationEvent {
            event_id: CurationEventId(id.to_owned()),
            chunk_id: ChunkId("a_p0".to_owned()),
            chunk_fingerprint: ChunkFingerprint("fp".to_owned()),
            decision: ReviewerDecision::Accepted,
            reviewer: ReviewerId("alice".to_owned()),
            occurred_at: "2001-09-09T01:46:40Z".to_owned(),
            corpus_fingerprint: CorpusFingerprint("corpus".to_owned()),
            context: CurationContext {
                cluster_representative: ChunkId("a_p0".to_owned()),
                cluster_members: vec![ChunkId("a_p0".to_owned())],
            },
            tags: Vec::new(),
            note: None,
        }
    }
}
