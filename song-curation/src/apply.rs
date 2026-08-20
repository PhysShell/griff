//! ADR-0033 **Slice 2** — transactional Apply.
//!
//! Executable implementation of the independently accepted Slice-2 contract
//! (`docs/proposals/song-curation-slice-2-transactional-apply.md`, normative
//! reviewed artifact `47e734cf…`, acceptance recorded in
//! `docs/decisions.log.md`). Apply consumes a serialized Slice-1
//! [`DryRunPlan`], the current corpus snapshot, and the application index,
//! and — under the contract's 12-step fail-closed verification order —
//! publishes the curated snapshot with its proof artifacts: the curated
//! manifest, the application report, and the appended index record.
//!
//! The batch is applied **iff** its record is in the application index
//! (§8.2): one publication `rename` makes the snapshot visible, and the
//! index temp+`rename` is the single commit point. Every refusal — I/O
//! included — is returned only by a run that did not reach that commit.

use crate::{
    corpus_fingerprint, inventory, replay, verify_plan, Action, CurationError, DryRunPlan,
};
use griff_core::corpus::{
    song_holdout_preflight, source_sha256, CorpusManifest, SongHoldoutRefusal, SongId,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

// ── artifact schemas (§5) ──────────────────────────────────────────────────────

/// Schema id of the application index (§5.1).
pub const APPLICATIONS_SCHEMA: &str = "song-curation.applications.v1";
/// Schema id of the application report (§5.2).
pub const REPORT_SCHEMA: &str = "song-curation.apply-report.v1";
/// The lockfile ownership marker (§8.1) — one fixed line.
pub const LOCK_MARKER: &str = "{\"schema\":\"song-curation.lock.v1\"}";
/// Fixed curated-manifest location inside the published snapshot (§4.3).
pub const CURATED_MANIFEST_RELPATH: &str = "song-curation/manifest.json";
/// Fixed application-report location inside the published snapshot (§4.3).
pub const REPORT_RELPATH: &str = "song-curation/apply-report.json";
/// The reserved area name inside every snapshot (§4.2).
pub const RESERVED_DIR: &str = "song-curation";

/// The append-only applied-batch registry (§5.1). Strict on the wire.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApplicationIndex {
    pub schema: String,
    pub applications: Vec<ApplicationRecord>,
}

/// One applied batch (§5.1). Content-addressed; deliberately no paths.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApplicationRecord {
    pub batch_id: String,
    pub report_digest: String,
    pub input_corpus_fingerprint: String,
    pub output_corpus_fingerprint: String,
}

/// The application report v1 (§5.2). Proof-bearing; no wall-clock timestamp.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApplicationReport {
    pub schema: String,
    pub batch_id: String,
    pub applied_event_ids: Vec<String>,
    pub input_corpus_fingerprint: String,
    pub output_corpus_fingerprint: String,
    pub decisions_digest: String,
    pub plan_digest: String,
    pub previous_application_report_digest: Option<String>,
    pub curated_manifest_path: String,
    pub curated_manifest_digest: String,
    pub assignments_applied: u64,
    pub assignments_unchanged: u64,
    pub sources_reviewed_unassigned: u64,
    pub sources_untouched: u64,
    pub coverage: Coverage,
    pub holdout_ready: bool,
    pub holdout_refusals: Vec<HoldoutRefusalRecord>,
    pub report_digest: String,
}

/// Post-apply totals over the output snapshot (§5.2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Coverage {
    pub unique_sources: u64,
    pub labelled: u64,
    pub unlabelled: u64,
    pub songs: u64,
}

/// One recorded holdout-preflight refusal on the curated view (§5.2), sorted
/// by the total tuple `(kind, sha256, song_id, chunk_id)`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HoldoutRefusalRecord {
    pub kind: String,
    pub sha256: Option<String>,
    pub song_id: Option<String>,
    pub chunk_id: Option<String>,
}

/// `report_digest` (§5.4): the shared Slice-1 canonical encoding over the
/// complete report with the `report_digest` field omitted.
#[must_use]
pub fn report_digest(report: &ApplicationReport) -> String {
    let mut value = serde_json::to_value(report).unwrap_or(Value::Null);
    if let Value::Object(map) = &mut value {
        map.remove("report_digest");
    }
    source_sha256(crate::canonical_json(&value).as_bytes())
}

// ── the Apply refusal surface (§12) ────────────────────────────────────────────

/// The closed Slice-2 refusal surface (§12): 24 new typed refusals plus the
/// Slice-1 refusals reused verbatim through step 5 — [`ApplyRefusal::PlanVerification`]
/// is transport for those, not a new refusal kind.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(clippy::module_name_repetitions)]
pub enum ApplyRefusal {
    OutputAlreadyExists {
        path: String,
    },
    OutputWouldModifyInput {
        detail: String,
    },
    ApplicationIndexInsideTree {
        path: String,
    },
    OutputCollidesWithIndexArtifacts {
        path: String,
        artifact: String,
    },
    OutputNameReserved {
        path: String,
    },
    ApplicationIndexHardLinked {
        path: String,
        nlink: u64,
    },
    ApplicationIndexLocked {
        path: String,
    },
    ApplicationIndexLockPathOccupied {
        path: String,
    },
    ApplicationIndexTempExists {
        path: String,
    },
    CuratedManifestPathNotDistinct {
        path: String,
    },
    MalformedPlanArtifact {
        detail: String,
    },
    MalformedApplicationIndex {
        detail: String,
    },
    CorpusTreeDisagreement {
        detail: String,
    },
    OrdinaryManifestCarriesSongs,
    UnsupportedApplicationIndexSchema {
        schema: String,
    },
    DuplicateAppliedBatchId {
        batch_id: String,
    },
    ApplicationIndexChainInvalid {
        position: usize,
        detail: String,
    },
    /// The Slice-1 refusal surface, reused verbatim through step 5 (§12).
    PlanVerification {
        refusals: Vec<CurationError>,
    },
    DecisionBatchAlreadyApplied {
        batch_id: String,
    },
    ApplicationChainMismatch {
        relation: String,
        expected: String,
        actual: String,
    },
    SupersessionEvidenceContradiction {
        event_id: String,
        detail: String,
    },
    ExistingLabelReplacementNotAuthorized {
        source_sha256: String,
        on_disk_song_id: String,
        new_song_id: String,
        event_id: String,
    },
    NonCanonicalCorpusFile {
        path: String,
        detail: String,
    },
    ApplyIoError {
        path: String,
        op: String,
        detail: String,
    },
    OutputPreflightInconsistent {
        detail: String,
    },
}

// ── the observable result shape (§8.2) ─────────────────────────────────────────

/// The four declared Apply inputs (§4.1).
#[derive(Debug, Clone)]
pub struct ApplyPaths {
    pub plan: PathBuf,
    pub corpus: PathBuf,
    pub index: PathBuf,
    pub output: PathBuf,
}

/// A committed application: the published report is the receipt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppliedReceipt {
    pub report: ApplicationReport,
}

/// A failed best-effort lock release — orthogonal to the primary outcome and
/// never a refusal (§8.2 result shape).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LockReleaseWarning {
    pub lockfile: String,
    pub detail: String,
}

/// Exactly one primary outcome plus the optional release warning (§8.2).
#[derive(Debug)]
pub struct ApplyRun {
    pub primary: Result<AppliedReceipt, ApplyRefusal>,
    pub lock_release_warning: Option<LockReleaseWarning>,
}

// ── entry point ────────────────────────────────────────────────────────────────

/// Run transactional Apply under the contract's 12-step verification order.
#[must_use]
pub fn apply(paths: &ApplyPaths) -> ApplyRun {
    // Step 1 (pre-lock): resolve the index identity and coordination names.
    let ctx = match preflight(paths) {
        Ok(ctx) => ctx,
        Err(refusal) => {
            return ApplyRun {
                primary: Err(refusal),
                lock_release_warning: None,
            }
        }
    };
    // Step 1 (lock): acquire the single-writer lock; held through step 12.
    if let Err(refusal) = acquire_lock(&ctx) {
        return ApplyRun {
            primary: Err(refusal),
            lock_release_warning: None,
        };
    }
    let primary = locked_apply(paths, &ctx);
    let lock_release_warning = release_lock(&ctx.lock_path);
    ApplyRun {
        primary,
        lock_release_warning,
    }
}

/// Everything step 1 resolves before any parsing.
struct Ctx {
    canonical_index: PathBuf,
    lock_path: PathBuf,
    temp_path: PathBuf,
    staging: PathBuf,
}

fn io_refusal(path: &Path, op: &str, err: &std::io::Error) -> ApplyRefusal {
    ApplyRefusal::ApplyIoError {
        path: path.display().to_string(),
        op: op.to_owned(),
        detail: err.to_string(),
    }
}

fn preflight(paths: &ApplyPaths) -> Result<Ctx, ApplyRefusal> {
    // The supplied index path must resolve to an existing regular file —
    // §4.1's missing-index refusal, raised before canonicalization and the
    // lock because `canonicalize` needs an existing target.
    let canonical_index =
        paths
            .index
            .canonicalize()
            .map_err(|e| ApplyRefusal::MalformedApplicationIndex {
                detail: format!(
                    "index {} does not resolve to an existing file: {e}",
                    paths.index.display()
                ),
            })?;
    let meta =
        fs::metadata(&canonical_index).map_err(|e| io_refusal(&canonical_index, "metadata", &e))?;
    if !meta.is_file() {
        return Err(ApplyRefusal::MalformedApplicationIndex {
            detail: format!("index {} is not a regular file", canonical_index.display()),
        });
    }
    let lock_path = coordination_path(&canonical_index, "lock");
    let temp_path = coordination_path(&canonical_index, "tmp");
    let staging = staging_path(&paths.output);
    Ok(Ctx {
        canonical_index,
        lock_path,
        temp_path,
        staging,
    })
}

/// `.<canonical_index_name>.<ext>` next to the canonical index file (§8.1).
fn coordination_path(canonical_index: &Path, ext: &str) -> PathBuf {
    let name = canonical_index
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    canonical_index
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(format!(".{name}.{ext}"))
}

/// `.<output_name>.apply-staging` as a sibling of the output path (§8.1).
fn staging_path(output: &Path) -> PathBuf {
    let name = output
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    output
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join(format!(".{name}.apply-staging"))
}

fn acquire_lock(ctx: &Ctx) -> Result<(), ApplyRefusal> {
    let mut file = match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&ctx.lock_path)
    {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            return Err(ApplyRefusal::ApplicationIndexLocked {
                path: ctx.lock_path.display().to_string(),
            });
        }
        Err(e) => return Err(io_refusal(&ctx.lock_path, "lock create_new", &e)),
    };
    if let Err(e) = file.write_all(LOCK_MARKER.as_bytes()) {
        let refusal = io_refusal(&ctx.lock_path, "lock marker write", &e);
        let _ = fs::remove_file(&ctx.lock_path);
        return Err(refusal);
    }
    Ok(())
}

fn release_lock(lock_path: &Path) -> Option<LockReleaseWarning> {
    match fs::remove_file(lock_path) {
        Ok(()) => None,
        Err(e) => Some(LockReleaseWarning {
            lockfile: lock_path.display().to_string(),
            detail: format!(
                "lock release failed ({e}); the stale lock refuses future applies until \
                 the §8.2 recovery removes it"
            ),
        }),
    }
}

// ── the locked phase: steps 2–12 ───────────────────────────────────────────────

#[allow(clippy::too_many_lines)]
fn locked_apply(paths: &ApplyPaths, ctx: &Ctx) -> Result<AppliedReceipt, ApplyRefusal> {
    // Step 2: strict artifact parsing, before any filesystem mutation.
    let plan_text =
        fs::read_to_string(&paths.plan).map_err(|e| ApplyRefusal::MalformedPlanArtifact {
            detail: format!("{}: {e}", paths.plan.display()),
        })?;
    let plan: DryRunPlan =
        serde_json::from_str(&plan_text).map_err(|e| ApplyRefusal::MalformedPlanArtifact {
            detail: format!("{}: {e}", paths.plan.display()),
        })?;
    let index_text = fs::read_to_string(&ctx.canonical_index).map_err(|e| {
        ApplyRefusal::MalformedApplicationIndex {
            detail: format!("{}: {e}", ctx.canonical_index.display()),
        }
    })?;
    let index: ApplicationIndex =
        serde_json::from_str(&index_text).map_err(|e| ApplyRefusal::MalformedApplicationIndex {
            detail: format!("{}: {e}", ctx.canonical_index.display()),
        })?;

    // Step 3: snapshot load, tree agreement, reserved-area shape, no-root-songs.
    let snapshot = load_snapshot(&paths.corpus)?;
    if snapshot.manifest.songs.is_some() {
        return Err(ApplyRefusal::OrdinaryManifestCarriesSongs);
    }

    // Step 4: application-index validation (§5.1).
    validate_index(&index)?;

    // Step 5: plan verification — the Slice-1 `verify_plan`, reused verbatim.
    verify_plan(&plan, &snapshot.manifest)
        .map_err(|refusals| ApplyRefusal::PlanVerification { refusals })?;

    // Step 6: already-applied — before the chain equations (§7.3).
    let batch = &plan.decision_batch;
    if index
        .applications
        .iter()
        .any(|r| r.batch_id == batch.batch_id)
    {
        return Err(ApplyRefusal::DecisionBatchAlreadyApplied {
            batch_id: batch.batch_id.clone(),
        });
    }

    // Step 7: the chain law (§7.1–§7.2).
    let current_fp = corpus_fingerprint(&snapshot.manifest);
    check_chain(
        batch.previous_application_report_digest.as_deref(),
        &batch.input_corpus_fingerprint,
        &index,
        &current_fp,
    )?;

    // Step 8: supersession-evidence consistency, then replacement authority.
    check_supersession_consistency(&plan)?;
    let counts = check_replacement_authority(&plan, &snapshot.manifest)?;

    // Steps 9–12: the filesystem protocol (§8).
    stage_publish_commit(
        paths,
        ctx,
        &StageInput {
            plan: &plan,
            index: &index,
            snapshot: &snapshot,
            counts: &counts,
        },
    )
}

/// Everything steps 2–8 verified, handed to the filesystem protocol.
struct StageInput<'a> {
    plan: &'a DryRunPlan,
    index: &'a ApplicationIndex,
    snapshot: &'a Snapshot,
    counts: &'a AuthorityCounts,
}

// ── step 3: the snapshot ───────────────────────────────────────────────────────

struct Snapshot {
    manifest: CorpusManifest,
    /// Every non-reserved file, as (relative path, absolute path), sorted.
    files: Vec<(String, PathBuf)>,
}

fn tree_disagreement(detail: String) -> ApplyRefusal {
    ApplyRefusal::CorpusTreeDisagreement { detail }
}

fn load_snapshot(corpus: &Path) -> Result<Snapshot, ApplyRefusal> {
    let manifest_path = corpus.join("manifest.json");
    let manifest_text = fs::read_to_string(&manifest_path)
        .map_err(|e| tree_disagreement(format!("root manifest.json: {e}")))?;
    let manifest: CorpusManifest = serde_json::from_str(&manifest_text)
        .map_err(|e| tree_disagreement(format!("root manifest.json: {e}")))?;

    // Recursive sorted walk, the migrate discipline; the reserved area is
    // excluded from corpus-content enumeration and shape-checked instead.
    let mut files = Vec::new();
    walk(corpus, corpus, &mut files).map_err(|(p, e)| io_refusal(&p, "walk", &e))?;
    files.sort();
    let reserved_root = corpus.join(RESERVED_DIR);
    if reserved_root.exists() {
        check_reserved_shape(corpus, &files)?;
    }
    let files: Vec<(String, PathBuf)> = files
        .into_iter()
        .filter(|(rel, _)| !is_reserved(rel))
        .collect();

    // Tree agreement: manifest chunk records == on-disk chunk records, as
    // multisets of canonical encodings (§4.2).
    let mut disk_chunks = Vec::new();
    for (rel, abs) in &files {
        if rel.ends_with(".chunk.json") {
            let text =
                fs::read_to_string(abs).map_err(|e| tree_disagreement(format!("{rel}: {e}")))?;
            let meta: griff_core::corpus::ChunkMeta = serde_json::from_str(&text)
                .map_err(|e| tree_disagreement(format!("{rel}: {e}")))?;
            disk_chunks.push(canonical_chunk(&meta));
        }
    }
    let mut manifest_chunks: Vec<String> = manifest.chunks.iter().map(canonical_chunk).collect();
    disk_chunks.sort();
    manifest_chunks.sort();
    if disk_chunks != manifest_chunks {
        let first = manifest_chunks
            .iter()
            .find(|c| !disk_chunks.contains(c))
            .or_else(|| disk_chunks.iter().find(|c| !manifest_chunks.contains(c)));
        return Err(tree_disagreement(format!(
            "manifest chunk records and on-disk chunk files disagree ({} records vs {} files); \
             first divergence: {}",
            manifest_chunks.len(),
            disk_chunks.len(),
            first.map_or_else(String::new, |c| truncate(c, 200)),
        )));
    }

    Ok(Snapshot { manifest, files })
}

fn is_reserved(rel: &str) -> bool {
    rel == RESERVED_DIR || rel.starts_with(&format!("{RESERVED_DIR}/"))
}

fn canonical_chunk(meta: &griff_core::corpus::ChunkMeta) -> String {
    crate::canonical_json(&serde_json::to_value(meta).unwrap_or(Value::Null))
}

fn truncate(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

fn walk(
    root: &Path,
    dir: &Path,
    out: &mut Vec<(String, PathBuf)>,
) -> Result<(), (PathBuf, std::io::Error)> {
    let entries = fs::read_dir(dir).map_err(|e| (dir.to_path_buf(), e))?;
    for entry in entries {
        let path = entry.map_err(|e| (dir.to_path_buf(), e))?.path();
        if path.is_dir() {
            walk(root, &path, out)?;
        } else {
            let rel = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            out.push((rel, path));
        }
    }
    Ok(())
}

/// The reserved-area shape law (§4.2): recursively, at most the two tool-owned
/// proof artifacts as regular files at the reserved root.
fn check_reserved_shape(_corpus: &Path, files: &[(String, PathBuf)]) -> Result<(), ApplyRefusal> {
    let allowed = [
        format!("{RESERVED_DIR}/manifest.json"),
        format!("{RESERVED_DIR}/apply-report.json"),
    ];
    for (rel, _) in files.iter().filter(|(rel, _)| is_reserved(rel)) {
        if !allowed.contains(rel) {
            return Err(tree_disagreement(format!(
                "foreign reserved-area entry: {rel}"
            )));
        }
    }
    Ok(())
}

// ── step 4: index validation (§5.1) ────────────────────────────────────────────

fn validate_index(index: &ApplicationIndex) -> Result<(), ApplyRefusal> {
    if index.schema != APPLICATIONS_SCHEMA {
        return Err(ApplyRefusal::UnsupportedApplicationIndexSchema {
            schema: index.schema.clone(),
        });
    }
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    for record in &index.applications {
        if !seen.insert(record.batch_id.as_str()) {
            return Err(ApplyRefusal::DuplicateAppliedBatchId {
                batch_id: record.batch_id.clone(),
            });
        }
    }
    for (i, pair) in index.applications.windows(2).enumerate() {
        if pair[1].input_corpus_fingerprint != pair[0].output_corpus_fingerprint {
            return Err(ApplyRefusal::ApplicationIndexChainInvalid {
                position: i + 1,
                detail: format!(
                    "applications[{}].input_corpus_fingerprint {} != applications[{}].output_corpus_fingerprint {}",
                    i + 1,
                    pair[1].input_corpus_fingerprint,
                    i,
                    pair[0].output_corpus_fingerprint
                ),
            });
        }
    }
    Ok(())
}

// ── step 7: the chain law (§7) ─────────────────────────────────────────────────

fn chain_mismatch(relation: &str, expected: String, actual: String) -> ApplyRefusal {
    ApplyRefusal::ApplicationChainMismatch {
        relation: relation.to_owned(),
        expected,
        actual,
    }
}

fn check_chain(
    prev: Option<&str>,
    batch_input_fp: &str,
    index: &ApplicationIndex,
    current_fp: &str,
) -> Result<(), ApplyRefusal> {
    match index.applications.last() {
        None => {
            // §7.1: exactly one relation for the initial batch.
            if let Some(actual) = prev {
                return Err(chain_mismatch(
                    "(initial) previous_application_report_digest == null",
                    "null".to_owned(),
                    actual.to_owned(),
                ));
            }
        }
        Some(head) => {
            // §7.2 relation (1).
            match prev {
                Some(p) if p == head.report_digest => {}
                other => {
                    return Err(chain_mismatch(
                        "previous_application_report_digest == head report_digest",
                        head.report_digest.clone(),
                        other.map_or_else(|| "null".to_owned(), ToOwned::to_owned),
                    ));
                }
            }
            // §7.2 relation (2).
            if batch_input_fp != head.output_corpus_fingerprint {
                return Err(chain_mismatch(
                    "batch input_corpus_fingerprint == head output_corpus_fingerprint",
                    head.output_corpus_fingerprint.clone(),
                    batch_input_fp.to_owned(),
                ));
            }
            // §7.2 relation (3).
            if head.output_corpus_fingerprint != current_fp {
                return Err(chain_mismatch(
                    "head output_corpus_fingerprint == current corpus fingerprint",
                    current_fp.to_owned(),
                    head.output_corpus_fingerprint.clone(),
                ));
            }
        }
    }
    Ok(())
}

// ── step 8: supersession consistency + replacement authority (§7.4) ────────────

fn sorted_unique(items: &[String]) -> Vec<String> {
    let mut v = items.to_vec();
    v.sort();
    v.dedup();
    v
}

fn check_supersession_consistency(plan: &DryRunPlan) -> Result<(), ApplyRefusal> {
    for event in &plan.decision_batch.events {
        let contradiction = |detail: String| ApplyRefusal::SupersessionEvidenceContradiction {
            event_id: event.event_id.clone(),
            detail,
        };
        match &event.action {
            Action::AcceptSuggestion {
                supersedes_song_ids,
                ..
            } => {
                if !supersedes_song_ids.is_empty() {
                    return Err(contradiction(format!(
                        "accept_suggestion must carry an empty supersedes_song_ids, got {supersedes_song_ids:?}"
                    )));
                }
            }
            Action::Merge {
                from_song_ids,
                supersedes_song_ids,
                ..
            } => {
                if sorted_unique(supersedes_song_ids) != sorted_unique(from_song_ids) {
                    return Err(contradiction(format!(
                        "merge supersedes_song_ids {supersedes_song_ids:?} != from_song_ids {from_song_ids:?}"
                    )));
                }
            }
            Action::Split {
                from_song_id,
                supersedes_song_ids,
                ..
            } => {
                if supersedes_song_ids != &vec![from_song_id.clone()] {
                    return Err(contradiction(format!(
                        "split supersedes_song_ids {supersedes_song_ids:?} != [{from_song_id:?}]"
                    )));
                }
            }
            Action::ManualDefine { .. }
            | Action::Correct { .. }
            | Action::RejectSuggestion { .. } => {}
        }
    }
    Ok(())
}

/// The four-way partition of the snapshot's sources (§5.2).
struct AuthorityCounts {
    applied: u64,
    unchanged: u64,
    reviewed_unassigned: u64,
    untouched: u64,
}

/// The authorized supersession set of the acting event (§7.4).
fn authorized_set(action: &Action) -> BTreeSet<&str> {
    match action {
        Action::Correct {
            supersedes_song_ids,
            ..
        } => supersedes_song_ids.iter().map(String::as_str).collect(),
        Action::Merge { from_song_ids, .. } => from_song_ids.iter().map(String::as_str).collect(),
        Action::Split { from_song_id, .. } => std::iter::once(from_song_id.as_str()).collect(),
        Action::AcceptSuggestion { .. }
        | Action::ManualDefine { .. }
        | Action::RejectSuggestion { .. } => BTreeSet::new(),
    }
}

fn check_replacement_authority(
    plan: &DryRunPlan,
    manifest: &CorpusManifest,
) -> Result<AuthorityCounts, ApplyRefusal> {
    // verify_plan (step 5) has proven the plan projection is reproducible,
    // so inventory and replay cannot fail here; the single shared replay
    // primitive also carries the acting-event attribution (§9).
    let inv =
        inventory(manifest).map_err(|refusals| ApplyRefusal::PlanVerification { refusals })?;
    let state = replay(&inv, &plan.decision_batch)
        .map_err(|refusals| ApplyRefusal::PlanVerification { refusals })?;
    let on_disk: BTreeMap<&str, Option<&str>> = inv
        .sources
        .iter()
        .map(|s| {
            (
                s.source_sha256.as_str(),
                s.existing_song_ids.first().map(String::as_str),
            )
        })
        .collect();
    let events: BTreeMap<&str, &Action> = plan
        .decision_batch
        .events
        .iter()
        .map(|e| (e.event_id.as_str(), &e.action))
        .collect();

    let mut counts = AuthorityCounts {
        applied: 0,
        unchanged: 0,
        reviewed_unassigned: 0,
        untouched: 0,
    };
    for (sha, effect) in &state {
        let Some(new_song) = &effect.song else {
            counts.reviewed_unassigned += 1;
            continue;
        };
        let disk = on_disk.get(sha.as_str()).copied().flatten();
        match disk {
            None => counts.applied += 1,
            Some(existing) if existing == new_song => counts.unchanged += 1,
            Some(existing) => {
                let action = events.get(effect.event_id.as_str()).copied();
                let authorized = action.is_some_and(|a| authorized_set(a).contains(existing));
                if !authorized {
                    return Err(ApplyRefusal::ExistingLabelReplacementNotAuthorized {
                        source_sha256: sha.clone(),
                        on_disk_song_id: existing.to_owned(),
                        new_song_id: new_song.clone(),
                        event_id: effect.event_id.clone(),
                    });
                }
                counts.applied += 1;
            }
        }
    }
    let total = u64::try_from(inv.sources.len()).unwrap_or(u64::MAX);
    counts.untouched = total - counts.applied - counts.unchanged - counts.reviewed_unassigned;
    Ok(counts)
}

// ── steps 9–12: the filesystem protocol (§8) ───────────────────────────────────

fn cleanup_staging(staging: &Path) {
    let _ = fs::remove_dir_all(staging);
}

#[allow(clippy::too_many_lines)]
fn stage_publish_commit(
    paths: &ApplyPaths,
    ctx: &Ctx,
    input: &StageInput<'_>,
) -> Result<AppliedReceipt, ApplyRefusal> {
    let StageInput {
        plan,
        index,
        snapshot,
        counts,
    } = *input;
    let assigned: BTreeMap<&str, &str> = plan
        .assignments
        .iter()
        .map(|a| (a.source_sha256.as_str(), a.song_id.as_str()))
        .collect();

    // Step 9: stage the snapshot — corpus files under the preservation law
    // (§10) plus the curated manifest. The report is written only in step 10.
    let staged = stage_snapshot(ctx, plan, snapshot, &assigned);
    let staged_manifest = match staged {
        Ok(m) => m,
        Err(refusal) => {
            cleanup_staging(&ctx.staging);
            return Err(refusal);
        }
    };

    // Step 10: staged self-check from bytes, the single preflight, then the
    // report.
    let step10 = staged_selfcheck_and_report(ctx, plan, counts, &staged_manifest);
    let report = match step10 {
        Ok(report) => report,
        Err(refusal) => {
            cleanup_staging(&ctx.staging);
            return Err(refusal);
        }
    };

    // Step 11: publish the snapshot with one rename.
    if let Err(e) = fs::rename(&ctx.staging, &paths.output) {
        let refusal = io_refusal(&paths.output, "publish rename", &e);
        cleanup_staging(&ctx.staging);
        return Err(refusal);
    }

    // Step 12: commit — temp write + rename over the canonical index. Only
    // after this rename is the batch applied.
    let mut updated = index.clone();
    updated.applications.push(ApplicationRecord {
        batch_id: report.batch_id.clone(),
        report_digest: report.report_digest.clone(),
        input_corpus_fingerprint: report.input_corpus_fingerprint.clone(),
        output_corpus_fingerprint: report.output_corpus_fingerprint.clone(),
    });
    let index_json =
        serde_json::to_string_pretty(&updated).map_err(|e| ApplyRefusal::ApplyIoError {
            path: ctx.temp_path.display().to_string(),
            op: "serialize index".to_owned(),
            detail: e.to_string(),
        })?;
    let mut temp = fs::File::create(&ctx.temp_path)
        .map_err(|e| io_refusal(&ctx.temp_path, "temp create", &e))?;
    temp.write_all(index_json.as_bytes())
        .map_err(|e| io_refusal(&ctx.temp_path, "temp write", &e))?;
    temp.sync_all()
        .map_err(|e| io_refusal(&ctx.temp_path, "temp sync", &e))?;
    drop(temp);
    fs::rename(&ctx.temp_path, &ctx.canonical_index).map_err(|e| {
        let refusal = io_refusal(&ctx.canonical_index, "commit rename", &e);
        let _ = fs::remove_file(&ctx.temp_path);
        refusal
    })?;

    Ok(AppliedReceipt { report })
}

/// Step 9: write the staged output tree. Returns the modified manifest.
fn stage_snapshot(
    ctx: &Ctx,
    plan: &DryRunPlan,
    snapshot: &Snapshot,
    assigned: &BTreeMap<&str, &str>,
) -> Result<CorpusManifest, ApplyRefusal> {
    fs::create_dir(&ctx.staging).map_err(|e| io_refusal(&ctx.staging, "staging create_dir", &e))?;

    let manifest_touched = !assigned.is_empty();
    let mut modified_manifest = snapshot.manifest.clone();
    for chunk in &mut modified_manifest.chunks {
        if let Some(sha) = &chunk.source.sha256 {
            if let Some(song) = assigned.get(sha.as_str()) {
                chunk.source.song_id = Some(SongId((*song).to_owned()));
            }
        }
    }

    for (rel, abs) in &snapshot.files {
        let dest = ctx.staging.join(rel);
        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(|e| io_refusal(parent, "staging mkdir", &e))?;
        }
        let is_manifest = rel == "manifest.json";
        let is_chunk = rel.ends_with(".chunk.json");
        let touched = if is_manifest {
            manifest_touched
        } else if is_chunk {
            let text = fs::read_to_string(abs).map_err(|e| io_refusal(abs, "read chunk", &e))?;
            let meta: griff_core::corpus::ChunkMeta = serde_json::from_str(&text)
                .map_err(|e| tree_disagreement(format!("{rel}: {e}")))?;
            meta.source
                .sha256
                .as_deref()
                .is_some_and(|sha| assigned.contains_key(sha))
        } else {
            false
        };
        if touched {
            write_touched(rel, abs, &dest, assigned, &modified_manifest)?;
        } else {
            let bytes = fs::read(abs).map_err(|e| io_refusal(abs, "read", &e))?;
            fs::write(&dest, bytes).map_err(|e| io_refusal(&dest, "write", &e))?;
        }
    }

    // The curated manifest (§5.3): the output manifest with the exact songs
    // projection at the protected distinct path.
    let curated_dir = ctx.staging.join(RESERVED_DIR);
    fs::create_dir_all(&curated_dir).map_err(|e| io_refusal(&curated_dir, "mkdir", &e))?;
    let mut curated = modified_manifest.clone();
    curated.songs = Some(
        plan.generated_songs_map
            .iter()
            .map(|(song, shas)| (SongId(song.clone()), shas.clone()))
            .collect(),
    );
    let curated_json =
        serde_json::to_string_pretty(&curated).map_err(|e| ApplyRefusal::ApplyIoError {
            path: CURATED_MANIFEST_RELPATH.to_owned(),
            op: "serialize curated manifest".to_owned(),
            detail: e.to_string(),
        })?;
    let curated_path = ctx.staging.join(CURATED_MANIFEST_RELPATH);
    fs::write(&curated_path, curated_json).map_err(|e| io_refusal(&curated_path, "write", &e))?;

    Ok(modified_manifest)
}

/// Rewrite one touched JSON file under the §10 preservation law: semantic
/// identity outside the assigned `source.song_id` members, canonical
/// rendering, fail-closed laundering guard.
fn write_touched(
    rel: &str,
    abs: &Path,
    dest: &Path,
    assigned: &BTreeMap<&str, &str>,
    modified_manifest: &CorpusManifest,
) -> Result<(), ApplyRefusal> {
    let text = fs::read_to_string(abs).map_err(|e| io_refusal(abs, "read", &e))?;
    let raw: Value =
        serde_json::from_str(&text).map_err(|e| tree_disagreement(format!("{rel}: {e}")))?;
    let rendered = if rel == "manifest.json" {
        // Guard the unmodified round-trip first (§10.3).
        let reparsed: CorpusManifest =
            serde_json::from_str(&text).map_err(|e| tree_disagreement(format!("{rel}: {e}")))?;
        guard_round_trip(rel, &raw, &serde_json::to_value(&reparsed))?;
        serde_json::to_string_pretty(modified_manifest)
    } else {
        let meta: griff_core::corpus::ChunkMeta =
            serde_json::from_str(&text).map_err(|e| tree_disagreement(format!("{rel}: {e}")))?;
        guard_round_trip(rel, &raw, &serde_json::to_value(&meta))?;
        let mut meta = meta;
        if let Some(sha) = meta.source.sha256.clone() {
            if let Some(song) = assigned.get(sha.as_str()) {
                meta.source.song_id = Some(SongId((*song).to_owned()));
            }
        }
        serde_json::to_string_pretty(&meta)
    };
    let rendered = rendered.map_err(|e| ApplyRefusal::ApplyIoError {
        path: rel.to_owned(),
        op: "serialize".to_owned(),
        detail: e.to_string(),
    })?;
    fs::write(dest, rendered).map_err(|e| io_refusal(dest, "write", &e))
}

fn guard_round_trip(
    rel: &str,
    raw: &Value,
    reserialized: &Result<Value, serde_json::Error>,
) -> Result<(), ApplyRefusal> {
    let non_canonical = |detail: String| ApplyRefusal::NonCanonicalCorpusFile {
        path: rel.to_owned(),
        detail,
    };
    let reserialized = reserialized
        .as_ref()
        .map_err(|e| non_canonical(format!("re-serialization failed: {e}")))?;
    if reserialized != raw {
        return Err(non_canonical(
            "parse→serialize round-trip diverges from the raw JSON value — the file would \
             be laundered by a rewrite (unknown member, or a value the parse re-renders \
             differently)"
                .to_owned(),
        ));
    }
    Ok(())
}

/// Step 10: re-read staged bytes, recompute the proofs, run the single
/// preflight over the curated view, then build and write the report.
fn staged_selfcheck_and_report(
    ctx: &Ctx,
    plan: &DryRunPlan,
    counts: &AuthorityCounts,
    expected_manifest: &CorpusManifest,
) -> Result<ApplicationReport, ApplyRefusal> {
    let inconsistent = |detail: String| ApplyRefusal::OutputPreflightInconsistent { detail };

    // Re-read the staged root manifest from bytes.
    let staged_manifest_text = fs::read_to_string(ctx.staging.join("manifest.json"))
        .map_err(|e| inconsistent(format!("staged manifest.json unreadable: {e}")))?;
    let staged_manifest: CorpusManifest = serde_json::from_str(&staged_manifest_text)
        .map_err(|e| inconsistent(format!("staged manifest.json unparseable: {e}")))?;
    let expected_value = serde_json::to_value(expected_manifest).unwrap_or(Value::Null);
    let staged_value = serde_json::to_value(&staged_manifest).unwrap_or(Value::Null);
    if expected_value != staged_value {
        return Err(inconsistent(
            "staged root manifest diverges from the derived output manifest".to_owned(),
        ));
    }
    let output_fp = corpus_fingerprint(&staged_manifest);

    // Re-read the staged curated manifest from bytes; its digest is over the
    // exact published bytes (§5.2).
    let curated_path = ctx.staging.join(CURATED_MANIFEST_RELPATH);
    let curated_bytes = fs::read(&curated_path)
        .map_err(|e| inconsistent(format!("staged curated manifest: {e}")))?;
    let curated_digest = source_sha256(&curated_bytes);
    let curated: CorpusManifest = serde_json::from_slice(&curated_bytes)
        .map_err(|e| inconsistent(format!("staged curated manifest unparseable: {e}")))?;

    // The single execution of the real core preflight (§11).
    let (holdout_ready, holdout_refusals) = match song_holdout_preflight(&curated) {
        Ok(()) => (true, Vec::new()),
        Err(refusals) => {
            let mut records = Vec::new();
            for refusal in refusals {
                match refusal {
                    SongHoldoutRefusal::UncuratedSource { sha256, example } => {
                        records.push(HoldoutRefusalRecord {
                            kind: "uncurated_source".to_owned(),
                            sha256: Some(sha256),
                            song_id: None,
                            chunk_id: Some(example.0),
                        });
                    }
                    other => {
                        return Err(inconsistent(format!(
                            "non-partiality preflight refusal on the staged curated view: {other:?}"
                        )));
                    }
                }
            }
            records.sort();
            (false, records)
        }
    };

    // Coverage over the curated output view.
    let inv = inventory(&curated)
        .map_err(|e| inconsistent(format!("staged curated view fails inventory: {e:?}")))?;
    let unique = u64::try_from(inv.sources.len()).unwrap_or(u64::MAX);
    let labelled = u64::try_from(
        inv.sources
            .iter()
            .filter(|s| !s.existing_song_ids.is_empty())
            .count(),
    )
    .unwrap_or(u64::MAX);
    let songs: BTreeSet<&String> = inv
        .sources
        .iter()
        .flat_map(|s| s.existing_song_ids.iter())
        .collect();

    let mut report = ApplicationReport {
        schema: REPORT_SCHEMA.to_owned(),
        batch_id: plan.decision_batch.batch_id.clone(),
        applied_event_ids: plan
            .decision_batch
            .events
            .iter()
            .map(|e| e.event_id.clone())
            .collect(),
        input_corpus_fingerprint: plan.input_corpus_fingerprint.clone(),
        output_corpus_fingerprint: output_fp,
        decisions_digest: plan.decisions_digest.clone(),
        plan_digest: plan.plan_digest.clone(),
        previous_application_report_digest: plan
            .decision_batch
            .previous_application_report_digest
            .clone(),
        curated_manifest_path: CURATED_MANIFEST_RELPATH.to_owned(),
        curated_manifest_digest: curated_digest,
        assignments_applied: counts.applied,
        assignments_unchanged: counts.unchanged,
        sources_reviewed_unassigned: counts.reviewed_unassigned,
        sources_untouched: counts.untouched,
        coverage: Coverage {
            unique_sources: unique,
            labelled,
            unlabelled: unique - labelled,
            songs: u64::try_from(songs.len()).unwrap_or(u64::MAX),
        },
        holdout_ready,
        holdout_refusals,
        report_digest: String::new(),
    };
    report.report_digest = report_digest(&report);

    let report_json =
        serde_json::to_string_pretty(&report).map_err(|e| ApplyRefusal::ApplyIoError {
            path: REPORT_RELPATH.to_owned(),
            op: "serialize report".to_owned(),
            detail: e.to_string(),
        })?;
    let report_path = ctx.staging.join(REPORT_RELPATH);
    fs::write(&report_path, report_json).map_err(|e| io_refusal(&report_path, "write", &e))?;

    Ok(report)
}
