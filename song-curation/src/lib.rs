//! Offline song-id curation — **Slice 1** (ADR-0033): the read-only decision &
//! validation core.
//!
//! Pipeline covered here, all read-only over synthetic/fixture inputs:
//! inventory (collapse chunks by exact `sha256`) → validate a batched decisions
//! ledger → **project** one unapplied batch into the complete post-plan
//! source→song state → serialize a **dry-run plan** that embeds the complete
//! ordered batch → **verify** that plan by re-deriving its projection and its
//! whole contract (`DecisionProjectionMismatch` if the plan asserts anything the
//! events + corpus do not produce).
//!
//! Explicitly **out of scope** for Slice 1 (later slices): any corpus write,
//! output tree, transactional apply, application report/index, manifest
//! generation, holdout-readiness execution over a changed corpus, suggestion
//! generation, and any CLI/cockpit integration. This crate is isolated
//! (ADR-0010) and not built by workspace CI.

use griff_core::corpus::CorpusManifest;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

// ── inventory ─────────────────────────────────────────────────────────────────

/// One source file (by exact `sha256`) with its deterministic, sorted/unique
/// display evidence and existing labels. The curation unit (ADR-0033 §3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SourceRecord {
    pub source_sha256: String,
    pub filenames: Vec<String>,
    pub titles: Vec<String>,
    pub formats: Vec<String>,
    pub chunk_ids: Vec<String>,
    pub existing_song_ids: Vec<String>,
    pub existing_manifest_membership: Vec<String>,
}


/// The read-only inventory: sources sorted by `sha256`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Inventory {
    pub sources: Vec<SourceRecord>,
}

// ── batched decisions ledger v1 ─────────────────────────────────────────────────

/// A versioned JSON ledger (not JSONL): a monotonic issuance counter and an
/// ordered list of immutable batches.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecisionsLedger {
    pub schema: String,
    pub next_song_seq: u64,
    pub batches: Vec<DecisionBatch>,
}

/// One application unit: events all made against a single `input_corpus_fingerprint`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecisionBatch {
    pub batch_id: String,
    pub input_corpus_fingerprint: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_application_report_digest: Option<String>,
    pub events: Vec<DecisionEvent>,
}

/// One immutable curator decision. The `events` array order is authoritative;
/// `ordinal` must equal the zero-based array position (checked).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DecisionEvent {
    pub event_id: String,
    pub ordinal: u64,
    pub curator: String,
    pub occurred_at: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    pub action: Action,
}

/// The six first-class curator actions; `correct` / `merge` / `split` are the
/// authorized replacements. `reject_suggestion` produces **no** assignment but is
/// still replayed (it validates its sources and can supersede an earlier pending
/// assignment within the batch).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Action {
    AcceptSuggestion {
        candidate_id: String,
        source_sha256s: Vec<String>,
        assign_song_id: String,
        #[serde(default)]
        supersedes_song_ids: Vec<String>,
    },
    RejectSuggestion {
        candidate_id: String,
        reviewed_source_sha256s: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
    },
    ManualDefine {
        source_sha256s: Vec<String>,
        assign_song_id: String,
    },
    Correct {
        source_sha256s: Vec<String>,
        new_song_id: String,
        #[serde(default)]
        supersedes_song_ids: Vec<String>,
    },
    Merge {
        from_song_ids: Vec<String>,
        into_song_id: String,
        source_sha256s: Vec<String>,
        #[serde(default)]
        supersedes_song_ids: Vec<String>,
    },
    Split {
        from_song_id: String,
        into: Vec<SplitTarget>,
        #[serde(default)]
        supersedes_song_ids: Vec<String>,
    },
}

/// One target group of a `split`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SplitTarget {
    pub assign_song_id: String,
    pub source_sha256s: Vec<String>,
}

// ── dry-run plan v1 ─────────────────────────────────────────────────────────────

/// One planned source-level assignment made by the batch, and the chunks it
/// touches.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Assignment {
    pub source_sha256: String,
    pub song_id: String,
    pub expected_existing_song_id: Option<String>,
    pub affected_chunk_ids: Vec<String>,
}

/// The serialized dry-run plan: it **embeds the complete ordered batch** so a
/// verifier can replay it, plus the batch's assignments and the **complete
/// post-plan** songs map (existing labels overlaid with the batch), and the
/// digests.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DryRunPlan {
    pub schema: String,
    pub policy_id: String,
    pub policy_version: String,
    pub input_corpus_fingerprint: String,
    pub decision_batch: DecisionBatch,
    pub decisions_digest: String,
    pub plan_digest: String,
    pub assignments: Vec<Assignment>,
    pub generated_songs_map: BTreeMap<String, Vec<String>>,
}


// ── typed refusals (Slice-1 subset) ─────────────────────────────────────────────

/// A typed curation refusal. Slice 1 covers inventory, ledger, projection, and
/// plan-verification failures; apply/index/manifest refusals belong to later
/// slices.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CurationError {
    UnidentifiedSource {
        chunk_id: String,
    },
    ConflictingExistingSongIds {
        source_sha256: String,
        song_ids: Vec<String>,
    },
    UnknownDecisionSource {
        source_sha256: String,
    },
    SourceAssignedToMultipleSongs {
        source_sha256: String,
        song_ids: Vec<String>,
    },
    InvalidDecisionBatchOrder {
        batch_id: String,
        detail: String,
    },
    DuplicateDecisionEventId {
        event_id: String,
    },
    BatchNotInLedger {
        batch_id: String,
    },
    DecisionBatchFingerprintMismatch {
        expected: String,
        actual: String,
    },
    DecisionDigestMismatch {
        expected: String,
        actual: String,
    },
    PlanDigestMismatch {
        expected: String,
        actual: String,
    },
    DecisionProjectionMismatch {
        detail: String,
    },
}

// ── public API (bodies land in GREEN) ───────────────────────────────────────────

#[allow(clippy::missing_errors_doc)]
pub fn inventory(manifest: &CorpusManifest) -> Result<Inventory, Vec<CurationError>> {
    let _ = manifest;
    todo!("inventory")
}

#[must_use]
pub fn corpus_fingerprint(manifest: &CorpusManifest) -> String {
    let _ = manifest;
    todo!("corpus_fingerprint")
}

#[must_use]
pub fn decisions_digest(batch: &DecisionBatch) -> String {
    let _ = batch;
    todo!("decisions_digest")
}

#[must_use]
pub fn plan_digest(plan: &DryRunPlan) -> String {
    let _ = plan;
    todo!("plan_digest")
}

#[allow(clippy::missing_errors_doc)]
pub fn validate_batch(batch: &DecisionBatch) -> Result<(), Vec<CurationError>> {
    let _ = batch;
    todo!("validate_batch")
}

#[allow(clippy::missing_errors_doc)]
pub fn validate_ledger(ledger: &DecisionsLedger) -> Result<(), Vec<CurationError>> {
    let _ = ledger;
    todo!("validate_ledger")
}

#[allow(clippy::missing_errors_doc)]
pub fn build_plan(
    manifest: &CorpusManifest,
    ledger: &DecisionsLedger,
    batch_id: &str,
) -> Result<DryRunPlan, Vec<CurationError>> {
    let _ = (manifest, ledger, batch_id);
    todo!("build_plan")
}

#[allow(clippy::missing_errors_doc)]
pub fn verify_plan(
    plan: &DryRunPlan,
    manifest: &CorpusManifest,
) -> Result<(), Vec<CurationError>> {
    let _ = (plan, manifest);
    todo!("verify_plan")
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;
    use griff_core::corpus::{ChunkId, ChunkMeta, SongId, SourceFormat, SCHEMA_VERSION};

    const V10_CHUNK: &str = r#"{
        "id": "x", "title": "T",
        "source": { "filename": "f.gp5", "format": "gp5", "bar_range": null },
        "tempo_bpm": 120.0, "ticks_per_quarter": 960, "time_signature": [4, 4],
        "tuning": "standard_e", "tags": [], "boundaries": [], "techniques": [],
        "quality_flags": [], "created_at": "2026-01-01T00:00:00Z",
        "updated_at": "2026-01-01T00:00:00Z"
    }"#;

    fn chunk(
        id: &str,
        sha: Option<&str>,
        song: Option<&str>,
        filename: &str,
        title: &str,
    ) -> ChunkMeta {
        let mut c: ChunkMeta = serde_json::from_str(V10_CHUNK).expect("fixture parses");
        c.id = ChunkId(id.to_owned());
        c.title = title.to_owned();
        c.source.filename = filename.to_owned();
        c.source.format = SourceFormat::Gp5;
        c.source.sha256 = sha.map(ToOwned::to_owned);
        c.source.song_id = song.map(|s| SongId(s.to_owned()));
        c
    }

    fn manifest(
        chunks: Vec<ChunkMeta>,
        songs: Option<BTreeMap<SongId, Vec<String>>>,
    ) -> CorpusManifest {
        CorpusManifest {
            schema_version: SCHEMA_VERSION,
            chunks,
            groups: Vec::new(),
            songs,
        }
    }

    fn event(id: &str, ordinal: u64, action: Action) -> DecisionEvent {
        DecisionEvent {
            event_id: id.to_owned(),
            ordinal,
            curator: "curator".to_owned(),
            occurred_at: "2026-07-29T00:00:00Z".to_owned(),
            note: None,
            action,
        }
    }

    fn accept(candidate: &str, shas: &[&str], song: &str) -> Action {
        Action::AcceptSuggestion {
            candidate_id: candidate.to_owned(),
            source_sha256s: shas.iter().map(|s| (*s).to_owned()).collect(),
            assign_song_id: song.to_owned(),
            supersedes_song_ids: Vec::new(),
        }
    }

    fn reject(candidate: &str, shas: &[&str]) -> Action {
        Action::RejectSuggestion {
            candidate_id: candidate.to_owned(),
            reviewed_source_sha256s: shas.iter().map(|s| (*s).to_owned()).collect(),
            reason: None,
        }
    }

    fn batch_for(manifest: &CorpusManifest, id: &str, events: Vec<DecisionEvent>) -> DecisionBatch {
        DecisionBatch {
            batch_id: id.to_owned(),
            input_corpus_fingerprint: corpus_fingerprint(manifest),
            previous_application_report_digest: None,
            events,
        }
    }

    fn ledger_of(batches: Vec<DecisionBatch>) -> DecisionsLedger {
        DecisionsLedger {
            schema: "song-curation.decisions.v1".to_owned(),
            next_song_seq: 1,
            batches,
        }
    }

    fn plan_for(
        m: &CorpusManifest,
        batch: DecisionBatch,
    ) -> Result<DryRunPlan, Vec<CurationError>> {
        let id = batch.batch_id.clone();
        build_plan(m, &ledger_of(vec![batch]), &id)
    }

    fn uncurated() -> CorpusManifest {
        manifest(
            vec![
                chunk("a1", Some("shaA"), None, "Song A.gp5", "Song A"),
                chunk("a2", Some("shaA"), None, "Song A.gp5", "Song A"),
                chunk("b1", Some("shaB"), None, "Song B.gp5", "Song B"),
            ],
            None,
        )
    }

    // ── inventory ──────────────────────────────────────────────────────────

    #[test]
    fn inventory_collapses_by_sha256() {
        let inv = inventory(&uncurated()).expect("clean");
        assert_eq!(inv.sources.len(), 2);
        let a = inv
            .sources
            .iter()
            .find(|s| s.source_sha256 == "shaA")
            .expect("shaA");
        assert_eq!(a.chunk_ids, vec!["a1".to_owned(), "a2".to_owned()]);
    }

    #[test]
    fn inventory_refuses_unidentified_source() {
        let m = manifest(vec![chunk("a1", None, None, "x.gp5", "X")], None);
        let errs = inventory(&m).expect_err("no sha256 must refuse");
        assert!(errs.iter().any(
            |e| matches!(e, CurationError::UnidentifiedSource { chunk_id } if chunk_id == "a1")
        ));
    }

    #[test]
    fn inventory_refuses_conflicting_existing_labels() {
        let m = manifest(
            vec![
                chunk("a1", Some("shaA"), Some("song1"), "x.gp5", "X"),
                chunk("a2", Some("shaA"), Some("song2"), "x.gp5", "X"),
            ],
            None,
        );
        let errs = inventory(&m).expect_err("split label must refuse");
        assert!(errs.iter().any(|e| matches!(e, CurationError::ConflictingExistingSongIds { source_sha256, .. } if source_sha256 == "shaA")));
    }

    #[test]
    fn corpus_fingerprint_is_order_insensitive() {
        let a = manifest(
            vec![
                chunk("a1", Some("shaA"), None, "A.gp5", "A"),
                chunk("b1", Some("shaB"), None, "B.gp5", "B"),
            ],
            None,
        );
        let b = manifest(
            vec![
                chunk("b1", Some("shaB"), None, "B.gp5", "B"),
                chunk("a1", Some("shaA"), None, "A.gp5", "A"),
            ],
            None,
        );
        assert_eq!(corpus_fingerprint(&a), corpus_fingerprint(&b));
    }

    // ── batch / ledger ordering ────────────────────────────────────────────

    #[test]
    fn validate_batch_refuses_non_positional_ordinal() {
        let m = uncurated();
        let mut b = batch_for(
            &m,
            "batch1",
            vec![event("ev0", 0, accept("g", &["shaA"], "song-1"))],
        );
        b.events[0].ordinal = 5;
        let errs = validate_batch(&b).expect_err("bad ordinal");
        assert!(errs
            .iter()
            .any(|e| matches!(e, CurationError::InvalidDecisionBatchOrder { .. })));
    }

    #[test]
    fn validate_batch_refuses_duplicate_event_id() {
        let m = uncurated();
        let b = batch_for(
            &m,
            "batch1",
            vec![
                event("dup", 0, accept("g", &["shaA"], "song-1")),
                event("dup", 1, accept("h", &["shaB"], "song-2")),
            ],
        );
        let errs = validate_batch(&b).expect_err("dup id");
        assert!(errs.iter().any(|e| matches!(e, CurationError::DuplicateDecisionEventId { event_id } if event_id == "dup")));
    }

    #[test]
    fn ledger_refuses_event_id_duplicated_across_batches() {
        // Two batches, each internally valid, but sharing one event_id.
        let m = uncurated();
        let b1 = batch_for(
            &m,
            "batch1",
            vec![event("shared", 0, accept("g", &["shaA"], "song-1"))],
        );
        let b2 = batch_for(
            &m,
            "batch2",
            vec![event("shared", 0, accept("h", &["shaB"], "song-2"))],
        );
        let errs =
            build_plan(&m, &ledger_of(vec![b1, b2]), "batch1").expect_err("cross-batch dup id");
        assert!(errs.iter().any(|e| matches!(e, CurationError::DuplicateDecisionEventId { event_id } if event_id == "shared")));
        // it fails before producing a plan.
    }

    // ── projection / build_plan ────────────────────────────────────────────

    #[test]
    fn build_plan_projects_assignments_and_songs_map() {
        let m = uncurated();
        let b = batch_for(
            &m,
            "batch1",
            vec![
                event("ev0", 0, accept("g1", &["shaA"], "song-000001")),
                event("ev1", 1, accept("g2", &["shaB"], "song-000002")),
            ],
        );
        let plan = plan_for(&m, b.clone()).expect("clean");
        let a = plan
            .assignments
            .iter()
            .find(|x| x.source_sha256 == "shaA")
            .expect("shaA");
        assert_eq!(a.song_id, "song-000001");
        assert_eq!(a.affected_chunk_ids, vec!["a1".to_owned(), "a2".to_owned()]);
        assert_eq!(a.expected_existing_song_id, None);
        assert_eq!(
            plan.generated_songs_map.get("song-000001"),
            Some(&vec!["shaA".to_owned()])
        );
        assert_eq!(
            plan.decision_batch, b,
            "plan embeds the full batch verbatim"
        );
    }

    #[test]
    fn build_plan_refuses_fingerprint_mismatch() {
        let m = uncurated();
        let mut b = batch_for(
            &m,
            "batch1",
            vec![event("ev0", 0, accept("g", &["shaA"], "song-1"))],
        );
        b.input_corpus_fingerprint = "stale".to_owned();
        let errs = plan_for(&m, b).expect_err("stale fingerprint");
        assert!(errs
            .iter()
            .any(|e| matches!(e, CurationError::DecisionBatchFingerprintMismatch { .. })));
    }

    #[test]
    fn build_plan_refuses_unknown_decision_source() {
        let m = uncurated();
        let b = batch_for(
            &m,
            "batch1",
            vec![event("ev0", 0, accept("g", &["shaZ"], "song-1"))],
        );
        let errs = plan_for(&m, b).expect_err("unknown source");
        assert!(errs.iter().any(|e| matches!(e, CurationError::UnknownDecisionSource { source_sha256 } if source_sha256 == "shaZ")));
    }

    #[test]
    fn build_plan_refuses_source_assigned_to_multiple_songs() {
        let m = uncurated();
        let split = Action::Split {
            from_song_id: "song-old".to_owned(),
            into: vec![
                SplitTarget {
                    assign_song_id: "song-1".to_owned(),
                    source_sha256s: vec!["shaA".to_owned()],
                },
                SplitTarget {
                    assign_song_id: "song-2".to_owned(),
                    source_sha256s: vec!["shaA".to_owned()],
                },
            ],
            supersedes_song_ids: vec!["song-old".to_owned()],
        };
        let b = batch_for(&m, "batch1", vec![event("ev0", 0, split)]);
        let errs = plan_for(&m, b).expect_err("double assignment");
        assert!(errs.iter().any(|e| matches!(e, CurationError::SourceAssignedToMultipleSongs { source_sha256, .. } if source_sha256 == "shaA")));
    }

    #[test]
    fn event_order_changes_digest_and_projection() {
        let m = uncurated();
        let mk = |first: &str, second: &str| {
            batch_for(
                &m,
                "batch1",
                vec![
                    event("ev0", 0, accept("g1", &["shaA"], first)),
                    event("ev1", 1, accept("g2", &["shaA"], second)),
                ],
            )
        };
        let b1 = mk("song-1", "song-2");
        let b2 = mk("song-2", "song-1");
        assert_ne!(
            decisions_digest(&b1),
            decisions_digest(&b2),
            "order must change the digest"
        );
        let p1 = plan_for(&m, b1).expect("ok");
        let p2 = plan_for(&m, b2).expect("ok");
        let s1 = p1
            .assignments
            .iter()
            .find(|x| x.source_sha256 == "shaA")
            .expect("a")
            .song_id
            .clone();
        let s2 = p2
            .assignments
            .iter()
            .find(|x| x.source_sha256 == "shaA")
            .expect("a")
            .song_id
            .clone();
        assert_eq!(s1, "song-2");
        assert_eq!(s2, "song-1");
    }

    // ── complete post-plan songs map (finding 1) ───────────────────────────

    #[test]
    fn songs_map_preserves_untouched_existing_labels() {
        // shaA already labelled song-old; the batch only touches shaB.
        let m = manifest(
            vec![
                chunk("a1", Some("shaA"), Some("song-old"), "A.gp5", "A"),
                chunk("b1", Some("shaB"), None, "B.gp5", "B"),
            ],
            None,
        );
        let b = batch_for(
            &m,
            "batch1",
            vec![event("ev0", 0, accept("g", &["shaB"], "song-new"))],
        );
        let plan = plan_for(&m, b).expect("clean");
        // The batch assigns only shaB, but the complete map keeps shaA→song-old.
        assert_eq!(plan.assignments.len(), 1);
        assert_eq!(
            plan.generated_songs_map.get("song-old"),
            Some(&vec!["shaA".to_owned()])
        );
        assert_eq!(
            plan.generated_songs_map.get("song-new"),
            Some(&vec!["shaB".to_owned()])
        );
    }

    #[test]
    fn correction_moves_source_between_songs_in_the_map() {
        // shaA labelled song-old; a correction moves it to song-new.
        let m = manifest(
            vec![chunk("a1", Some("shaA"), Some("song-old"), "A.gp5", "A")],
            None,
        );
        let correct = Action::Correct {
            source_sha256s: vec!["shaA".to_owned()],
            new_song_id: "song-new".to_owned(),
            supersedes_song_ids: vec!["song-old".to_owned()],
        };
        let b = batch_for(&m, "batch1", vec![event("ev0", 0, correct)]);
        let plan = plan_for(&m, b).expect("clean");
        assert_eq!(
            plan.generated_songs_map.get("song-new"),
            Some(&vec!["shaA".to_owned()])
        );
        assert_eq!(
            plan.generated_songs_map.get("song-old"),
            None,
            "old song loses its only member"
        );
        let a = plan
            .assignments
            .iter()
            .find(|x| x.source_sha256 == "shaA")
            .expect("shaA");
        assert_eq!(a.expected_existing_song_id, Some("song-old".to_owned()));
    }

    #[test]
    fn songs_map_is_deterministic_under_chunk_permutation() {
        let base = |chunks: Vec<ChunkMeta>| {
            let m = manifest(chunks, None);
            let b = batch_for(
                &m,
                "batch1",
                vec![event("ev0", 0, accept("g", &["shaB"], "song-new"))],
            );
            plan_for(&m, b).expect("clean").generated_songs_map
        };
        let one = base(vec![
            chunk("a1", Some("shaA"), Some("song-old"), "A.gp5", "A"),
            chunk("b1", Some("shaB"), None, "B.gp5", "B"),
        ]);
        let two = base(vec![
            chunk("b1", Some("shaB"), None, "B.gp5", "B"),
            chunk("a1", Some("shaA"), Some("song-old"), "A.gp5", "A"),
        ]);
        assert_eq!(one, two);
    }

    // ── reject semantics (finding 2) ───────────────────────────────────────

    #[test]
    fn accept_then_reject_produces_no_assignment() {
        let m = uncurated();
        let b = batch_for(
            &m,
            "batch1",
            vec![
                event("ev0", 0, accept("g", &["shaA"], "song-1")),
                event("ev1", 1, reject("g", &["shaA"])),
            ],
        );
        let plan = plan_for(&m, b).expect("valid");
        assert!(
            plan.assignments.iter().all(|a| a.source_sha256 != "shaA"),
            "reject supersedes the earlier accept"
        );
        assert_eq!(plan.generated_songs_map.get("song-1"), None);
    }

    #[test]
    fn reject_then_accept_produces_the_later_assignment() {
        let m = uncurated();
        let b = batch_for(
            &m,
            "batch1",
            vec![
                event("ev0", 0, reject("g", &["shaA"])),
                event("ev1", 1, accept("g", &["shaA"], "song-1")),
            ],
        );
        let plan = plan_for(&m, b).expect("valid");
        let a = plan
            .assignments
            .iter()
            .find(|x| x.source_sha256 == "shaA")
            .expect("shaA assigned");
        assert_eq!(a.song_id, "song-1");
    }

    #[test]
    fn reject_naming_unknown_source_refuses() {
        let m = uncurated();
        let b = batch_for(&m, "batch1", vec![event("ev0", 0, reject("g", &["shaZ"]))]);
        let errs = plan_for(&m, b).expect_err("reject of unknown source");
        assert!(errs.iter().any(|e| matches!(e, CurationError::UnknownDecisionSource { source_sha256 } if source_sha256 == "shaZ")));
    }

    #[test]
    fn reject_only_batch_is_a_valid_fingerprint_neutral_decision() {
        let m = uncurated();
        let b = batch_for(&m, "batch1", vec![event("ev0", 0, reject("g", &["shaA"]))]);
        let plan = plan_for(&m, b).expect("reject-only batch is valid");
        assert!(plan.assignments.is_empty());
        assert!(
            plan.generated_songs_map.is_empty(),
            "uncurated corpus, no assignment → empty map"
        );
    }

    // ── verify_plan (proof by rederivation) ────────────────────────────────

    fn good_plan() -> (CorpusManifest, DryRunPlan) {
        let m = uncurated();
        let b = batch_for(
            &m,
            "batch1",
            vec![event("ev0", 0, accept("g1", &["shaA"], "song-000001"))],
        );
        let plan = plan_for(&m, b).expect("clean");
        (m, plan)
    }

    #[test]
    fn verify_plan_accepts_a_faithful_plan() {
        let (m, plan) = good_plan();
        assert_eq!(verify_plan(&plan, &m), Ok(()));
    }

    #[test]
    fn verify_plan_refuses_tampered_plan_digest() {
        let (m, mut plan) = good_plan();
        plan.plan_digest = "deadbeef".to_owned();
        let errs = verify_plan(&plan, &m).expect_err("tampered plan_digest");
        assert!(errs
            .iter()
            .any(|e| matches!(e, CurationError::PlanDigestMismatch { .. })));
    }

    #[test]
    fn verify_plan_refuses_tampered_decisions_digest() {
        let (m, mut plan) = good_plan();
        plan.decisions_digest = "deadbeef".to_owned();
        plan.plan_digest = plan_digest(&plan);
        let errs = verify_plan(&plan, &m).expect_err("tampered decisions_digest");
        assert!(errs
            .iter()
            .any(|e| matches!(e, CurationError::DecisionDigestMismatch { .. })));
    }

    #[test]
    fn verify_plan_refuses_forged_assignments_even_with_self_consistent_digests() {
        let (m, mut plan) = good_plan();
        plan.assignments[0].song_id = "song-forged".to_owned();
        plan.plan_digest = plan_digest(&plan);
        let errs = verify_plan(&plan, &m).expect_err("forged assignment");
        assert!(errs
            .iter()
            .any(|e| matches!(e, CurationError::DecisionProjectionMismatch { .. })));
    }

    #[test]
    fn verify_plan_refuses_forged_songs_map_even_with_self_consistent_digests() {
        let (m, mut plan) = good_plan();
        plan.generated_songs_map
            .insert("song-ghost".to_owned(), vec!["shaB".to_owned()]);
        plan.plan_digest = plan_digest(&plan);
        let errs = verify_plan(&plan, &m).expect_err("forged songs map");
        assert!(errs
            .iter()
            .any(|e| matches!(e, CurationError::DecisionProjectionMismatch { .. })));
    }

    #[test]
    fn verify_plan_refuses_forged_top_level_fingerprint() {
        // The embedded batch still replays correctly, but the plan's top-level
        // corpus binding is forged — the verifier must catch the contradiction.
        let (m, mut plan) = good_plan();
        plan.input_corpus_fingerprint = "forged".to_owned();
        plan.plan_digest = plan_digest(&plan);
        let errs = verify_plan(&plan, &m).expect_err("forged top-level fingerprint");
        assert!(errs
            .iter()
            .any(|e| matches!(e, CurationError::DecisionBatchFingerprintMismatch { .. })));
    }
}
