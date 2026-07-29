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

use griff_core::corpus::{source_sha256, CorpusManifest, SourceFormat};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

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

impl SourceRecord {
    /// The single existing label, or `None` when uncurated. A source with more
    /// than one is rejected by [`inventory`], so `first` is authoritative here.
    fn existing_label(&self) -> Option<String> {
        self.existing_song_ids.first().cloned()
    }
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
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

const PLAN_SCHEMA: &str = "song-curation.plan.v1";
const POLICY_ID: &str = "decision-core";
const POLICY_VERSION: &str = "1";
/// The one decisions-ledger schema this slice reads; anything else is refused.
const DECISIONS_SCHEMA: &str = "song-curation.decisions.v1";

/// Per-source batch state after replay: `Some(song)` = the batch assigns this
/// label; `None` = the batch reviewed the source without assigning (a `reject`
/// that supersedes any earlier pending assignment).
type BatchState = BTreeMap<String, Option<String>>;

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
    DuplicateDecisionBatchId {
        batch_id: String,
    },
    UnsupportedDecisionsLedgerSchema {
        schema: String,
    },
    BatchNotInLedger {
        batch_id: String,
    },
    /// The plan's **top-level** `input_corpus_fingerprint` disagrees with the
    /// corpus; `actual` carries the plan's own (rejected) value.
    PlanCorpusFingerprintMismatch {
        expected: String,
        actual: String,
    },
    /// The plan's **embedded batch** `input_corpus_fingerprint` disagrees with
    /// the corpus; `actual` carries the embedded batch's own (rejected) value.
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

// ── canonical JSON + digests ────────────────────────────────────────────────────

/// Emit `value` as compact UTF-8 JSON with object keys sorted lexicographically
/// and array order preserved — the shared canonical encoding for every digest.
fn canonical_json(value: &Value) -> String {
    let mut out = String::new();
    write_canonical(value, &mut out);
    out
}

fn write_canonical(value: &Value, out: &mut String) {
    match value {
        Value::Object(map) => {
            let mut entries: Vec<(&String, &Value)> = map.iter().collect();
            entries.sort_by(|a, b| a.0.cmp(b.0));
            out.push('{');
            for (i, (key, val)) in entries.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&scalar(&Value::String((*key).clone())));
                out.push(':');
                write_canonical(val, out);
            }
            out.push('}');
        }
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_canonical(item, out);
            }
            out.push(']');
        }
        scalar_value => out.push_str(&scalar(scalar_value)),
    }
}

/// Serialize a scalar `Value` to its JSON literal; scalars never fail.
fn scalar(value: &Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "null".to_owned())
}

fn opt_str(value: Option<&String>) -> Value {
    value.map_or(Value::Null, |s| Value::String(s.clone()))
}

fn format_label(format: SourceFormat) -> String {
    match format {
        SourceFormat::Midi => "midi",
        SourceFormat::Gp3 => "gp3",
        SourceFormat::Gp4 => "gp4",
        SourceFormat::Gp5 => "gp5",
        SourceFormat::Gpx => "gpx",
        SourceFormat::Gp => "gp",
    }
    .to_owned()
}

// ── inventory ─────────────────────────────────────────────────────────────────

/// Collapse a corpus snapshot's chunks by exact `sha256` into a deterministic,
/// read-only inventory.
///
/// # Errors
///
/// [`CurationError::UnidentifiedSource`] for a chunk with no `sha256`, and
/// [`CurationError::ConflictingExistingSongIds`] for a `sha256` carrying more
/// than one existing `song_id`.
pub fn inventory(manifest: &CorpusManifest) -> Result<Inventory, Vec<CurationError>> {
    #[derive(Default)]
    struct Accum {
        filenames: BTreeSet<String>,
        titles: BTreeSet<String>,
        formats: BTreeSet<String>,
        chunk_ids: BTreeSet<String>,
        song_ids: BTreeSet<String>,
    }

    let mut errors = Vec::new();
    let mut groups: BTreeMap<String, Accum> = BTreeMap::new();
    for chunk in &manifest.chunks {
        let Some(sha) = &chunk.source.sha256 else {
            errors.push(CurationError::UnidentifiedSource {
                chunk_id: chunk.id.0.clone(),
            });
            continue;
        };
        let group = groups.entry(sha.clone()).or_default();
        group.filenames.insert(chunk.source.filename.clone());
        group.titles.insert(chunk.title.clone());
        group.formats.insert(format_label(chunk.source.format));
        group.chunk_ids.insert(chunk.id.0.clone());
        if let Some(song) = &chunk.source.song_id {
            group.song_ids.insert(song.0.clone());
        }
    }

    let mut membership: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    if let Some(songs) = &manifest.songs {
        for (song, shas) in songs {
            for sha in shas {
                membership
                    .entry(sha.clone())
                    .or_default()
                    .insert(song.0.clone());
            }
        }
    }

    let mut sources = Vec::new();
    for (sha, group) in groups {
        if group.song_ids.len() > 1 {
            errors.push(CurationError::ConflictingExistingSongIds {
                source_sha256: sha.clone(),
                song_ids: group.song_ids.iter().cloned().collect(),
            });
        }
        let membership_of = membership
            .get(&sha)
            .map(|m| m.iter().cloned().collect())
            .unwrap_or_default();
        sources.push(SourceRecord {
            source_sha256: sha,
            filenames: group.filenames.into_iter().collect(),
            titles: group.titles.into_iter().collect(),
            formats: group.formats.into_iter().collect(),
            chunk_ids: group.chunk_ids.into_iter().collect(),
            existing_song_ids: group.song_ids.into_iter().collect(),
            existing_manifest_membership: membership_of,
        });
    }
    if errors.is_empty() {
        Ok(Inventory { sources })
    } else {
        Err(errors)
    }
}

// ── fingerprints / digests ──────────────────────────────────────────────────────

/// Order-insensitive corpus fingerprint over the label-bearing inputs (§9).
#[must_use]
pub fn corpus_fingerprint(manifest: &CorpusManifest) -> String {
    let mut records: Vec<String> = Vec::new();
    let presence = if manifest.songs.is_some() {
        "present"
    } else {
        "absent"
    };
    records.push(canonical_json(&json!(["manifest_songs", presence])));
    for chunk in &manifest.chunks {
        records.push(canonical_json(&json!([
            "chunk",
            chunk.id.0,
            opt_str(chunk.source.sha256.as_ref()),
            opt_str(chunk.source.song_id.as_ref().map(|s| &s.0)),
        ])));
    }
    if let Some(songs) = &manifest.songs {
        for (song, shas) in songs {
            let mut sorted = shas.clone();
            sorted.sort();
            sorted.dedup();
            records.push(canonical_json(&json!(["song", song.0, sorted])));
        }
    }
    records.sort();
    source_sha256(records.join("\n").as_bytes())
}

/// Order-**sensitive** digest of a decision batch (events kept in append order).
#[must_use]
pub fn decisions_digest(batch: &DecisionBatch) -> String {
    let value = serde_json::to_value(batch).unwrap_or(Value::Null);
    source_sha256(canonical_json(&value).as_bytes())
}

/// Order-aware digest of a plan, computed with the `plan_digest` field omitted.
#[must_use]
pub fn plan_digest(plan: &DryRunPlan) -> String {
    let mut value = serde_json::to_value(plan).unwrap_or(Value::Null);
    if let Value::Object(map) = &mut value {
        map.remove("plan_digest");
    }
    source_sha256(canonical_json(&value).as_bytes())
}

// ── validation + projection ─────────────────────────────────────────────────────

/// Validate one batch's ordering invariants: array order is authoritative,
/// `ordinal` == position, and `event_id`s are unique **within the batch**.
///
/// # Errors
///
/// [`CurationError::InvalidDecisionBatchOrder`] and
/// [`CurationError::DuplicateDecisionEventId`].
pub fn validate_batch(batch: &DecisionBatch) -> Result<(), Vec<CurationError>> {
    let mut errors = Vec::new();
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    for (index, event) in batch.events.iter().enumerate() {
        let position = u64::try_from(index).unwrap_or(u64::MAX);
        if event.ordinal != position {
            errors.push(CurationError::InvalidDecisionBatchOrder {
                batch_id: batch.batch_id.clone(),
                detail: format!(
                    "event {} has ordinal {}, expected {position}",
                    event.event_id, event.ordinal
                ),
            });
        }
        if !seen.insert(event.event_id.as_str()) {
            errors.push(CurationError::DuplicateDecisionEventId {
                event_id: event.event_id.clone(),
            });
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// Validate the whole ledger's identity and structure: the ledger `schema` is
/// the one this slice reads, `batch_id`s are unique (so batch selection is
/// unambiguous), every batch's ordering holds, and `event_id`s are unique
/// **across all batches** (not merely within each).
///
/// `next_song_seq` is not validated here: song-id *issuance* is deferred to a
/// later slice, so this slice reads the counter but does not enforce its
/// semantics.
///
/// # Errors
///
/// [`CurationError::UnsupportedDecisionsLedgerSchema`],
/// [`CurationError::DuplicateDecisionBatchId`],
/// [`CurationError::InvalidDecisionBatchOrder`], and
/// [`CurationError::DuplicateDecisionEventId`] (ledger-wide).
pub fn validate_ledger(ledger: &DecisionsLedger) -> Result<(), Vec<CurationError>> {
    let mut errors = Vec::new();
    if ledger.schema != DECISIONS_SCHEMA {
        errors.push(CurationError::UnsupportedDecisionsLedgerSchema {
            schema: ledger.schema.clone(),
        });
    }
    let mut seen_events: BTreeSet<&str> = BTreeSet::new();
    let mut seen_batches: BTreeSet<&str> = BTreeSet::new();
    for batch in &ledger.batches {
        if !seen_batches.insert(batch.batch_id.as_str()) {
            errors.push(CurationError::DuplicateDecisionBatchId {
                batch_id: batch.batch_id.clone(),
            });
        }
        if let Err(mut e) = validate_batch(batch) {
            errors.append(&mut e);
        }
        for event in &batch.events {
            if !seen_events.insert(event.event_id.as_str()) {
                errors.push(CurationError::DuplicateDecisionEventId {
                    event_id: event.event_id.clone(),
                });
            }
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

/// The `(source_sha256, effect)` list one action produces, where `effect` is
/// `Some(song)` for a label assignment and `None` for a `reject` (a validated
/// review that clears any earlier pending assignment).
fn action_effects(action: &Action) -> Vec<(String, Option<String>)> {
    let assign = |shas: &[String], song: &str| -> Vec<(String, Option<String>)> {
        shas.iter()
            .map(|s| (s.clone(), Some(song.to_owned())))
            .collect()
    };
    match action {
        Action::AcceptSuggestion {
            source_sha256s,
            assign_song_id,
            ..
        }
        | Action::ManualDefine {
            source_sha256s,
            assign_song_id,
        } => assign(source_sha256s, assign_song_id),
        Action::Correct {
            source_sha256s,
            new_song_id,
            ..
        } => assign(source_sha256s, new_song_id),
        Action::Merge {
            source_sha256s,
            into_song_id,
            ..
        } => assign(source_sha256s, into_song_id),
        Action::Split { into, .. } => into
            .iter()
            .flat_map(|t| assign(&t.source_sha256s, &t.assign_song_id))
            .collect(),
        Action::RejectSuggestion {
            reviewed_source_sha256s,
            ..
        } => reviewed_source_sha256s
            .iter()
            .map(|s| (s.clone(), None))
            .collect(),
    }
}

/// Replay a batch's events (append order, latest wins) into a per-source batch
/// state: `Some(song)` where the batch assigns a label, `None` where it reviewed
/// the source without assigning (a reject that supersedes any earlier pending
/// assignment). Every referenced source is validated; a source assigned two
/// distinct labels within one event refuses.
fn replay(inventory: &Inventory, batch: &DecisionBatch) -> Result<BatchState, Vec<CurationError>> {
    let known: BTreeSet<&str> = inventory
        .sources
        .iter()
        .map(|s| s.source_sha256.as_str())
        .collect();
    let mut errors = Vec::new();
    let mut state: BatchState = BTreeMap::new();
    for event in &batch.events {
        let effects = action_effects(&event.action);
        let mut within: BTreeMap<&str, BTreeSet<&str>> = BTreeMap::new();
        for (sha, effect) in &effects {
            if let Some(song) = effect {
                within.entry(sha).or_default().insert(song);
            }
        }
        for (sha, songs) in &within {
            if songs.len() > 1 {
                errors.push(CurationError::SourceAssignedToMultipleSongs {
                    source_sha256: (*sha).to_owned(),
                    song_ids: songs.iter().map(|s| (*s).to_owned()).collect(),
                });
            }
        }
        for (sha, effect) in effects {
            if known.contains(sha.as_str()) {
                state.insert(sha, effect);
            } else {
                errors.push(CurationError::UnknownDecisionSource { source_sha256: sha });
            }
        }
    }
    if errors.is_empty() {
        Ok(state)
    } else {
        Err(errors)
    }
}

/// The derived body of a plan: the batch's assignments and the **complete
/// post-plan** songs map (existing labels overlaid with the batch state).
struct PlanBody {
    assignments: Vec<Assignment>,
    generated_songs_map: BTreeMap<String, Vec<String>>,
}

/// Inventory + replay one batch, then derive the assignments and the complete
/// post-plan songs map. Shared by [`build_plan`] and [`verify_plan`] so both
/// derive identically.
///
/// Precondition: `batch` has already passed [`validate_batch`] — via
/// [`validate_ledger`] in `build_plan`, or a direct `validate_batch` in
/// `verify_plan`. This function performs **no** ordering re-validation (that
/// would double-emit the same refusal); it only projects. It still validates
/// projection-level facts through [`replay`] (unknown sources, a source
/// assigned two labels in one event).
fn derive(
    manifest: &CorpusManifest,
    batch: &DecisionBatch,
) -> Result<PlanBody, Vec<CurationError>> {
    let inventory = inventory(manifest)?;
    let state = replay(&inventory, batch)?;

    let by_sha: BTreeMap<&str, &SourceRecord> = inventory
        .sources
        .iter()
        .map(|s| (s.source_sha256.as_str(), s))
        .collect();

    // Assignments: sources the batch actually labels (state == Some).
    let mut assignments: Vec<Assignment> = state
        .iter()
        .filter_map(|(sha, effect)| {
            let song = effect.clone()?;
            let record = by_sha.get(sha.as_str());
            Some(Assignment {
                source_sha256: sha.clone(),
                song_id: song,
                expected_existing_song_id: record.and_then(|r| r.existing_label()),
                affected_chunk_ids: record.map(|r| r.chunk_ids.clone()).unwrap_or_default(),
            })
        })
        .collect();
    assignments.sort_by(|a, b| a.source_sha256.cmp(&b.source_sha256));

    // Complete post-plan songs map: every source's final label — the batch
    // assignment when present, else its existing authoritative label — so
    // untouched labels survive (the map is the Slice-2 manifest projection).
    let mut generated_songs_map: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for source in &inventory.sources {
        let batch_label = state.get(&source.source_sha256).and_then(Clone::clone);
        if let Some(song) = batch_label.or_else(|| source.existing_label()) {
            generated_songs_map
                .entry(song)
                .or_default()
                .push(source.source_sha256.clone());
        }
    }
    for shas in generated_songs_map.values_mut() {
        shas.sort();
        shas.dedup();
    }

    Ok(PlanBody {
        assignments,
        generated_songs_map,
    })
}

/// Build the serialized dry-run plan for one unapplied batch of a validated
/// ledger.
///
/// # Errors
///
/// Whole-ledger validation refusals (schema, duplicate `batch_id`, ordering,
/// duplicate `event_id`) — reported **before** batch selection or projection;
/// then [`CurationError::BatchNotInLedger`],
/// [`CurationError::DecisionBatchFingerprintMismatch`], any inventory refusal, or
/// a replay refusal ([`CurationError::UnknownDecisionSource`] /
/// [`CurationError::SourceAssignedToMultipleSongs`]).
pub fn build_plan(
    manifest: &CorpusManifest,
    ledger: &DecisionsLedger,
    batch_id: &str,
) -> Result<DryRunPlan, Vec<CurationError>> {
    // Whole-ledger identity/structure first. A structurally invalid ledger
    // refuses here — before any batch is selected or projected — so ordering /
    // duplicate-id / schema faults never reach fingerprinting or replay, and
    // each is emitted exactly once (derive no longer re-validates).
    validate_ledger(ledger)?;

    // The ledger is valid, so `batch_id` is unique by construction: `find`
    // returns the one batch, never an ambiguous first-of-many.
    let Some(batch) = ledger.batches.iter().find(|b| b.batch_id == batch_id) else {
        return Err(vec![CurationError::BatchNotInLedger {
            batch_id: batch_id.to_owned(),
        }]);
    };

    let mut errors = Vec::new();
    let fingerprint = corpus_fingerprint(manifest);
    if batch.input_corpus_fingerprint != fingerprint {
        errors.push(CurationError::DecisionBatchFingerprintMismatch {
            expected: fingerprint.clone(),
            actual: batch.input_corpus_fingerprint.clone(),
        });
    }
    let body = match derive(manifest, batch) {
        Ok(b) => b,
        Err(mut e) => {
            errors.append(&mut e);
            return Err(errors);
        }
    };
    if !errors.is_empty() {
        return Err(errors);
    }

    let mut plan = DryRunPlan {
        schema: PLAN_SCHEMA.to_owned(),
        policy_id: POLICY_ID.to_owned(),
        policy_version: POLICY_VERSION.to_owned(),
        input_corpus_fingerprint: fingerprint,
        decision_batch: batch.clone(),
        decisions_digest: decisions_digest(batch),
        plan_digest: String::new(),
        assignments: body.assignments,
        generated_songs_map: body.generated_songs_map,
    };
    plan.plan_digest = plan_digest(&plan);
    Ok(plan)
}

/// Verify a dry-run plan by re-deriving its **whole contract**: recompute both
/// digests; re-derive the fingerprint, schema/policy, assignments, and complete
/// songs map from the embedded batch + corpus; and refuse
/// [`CurationError::DecisionProjectionMismatch`] on any divergence — so a plan
/// cannot rubber-stamp itself even with self-consistent digests.
///
/// # Errors
///
/// [`CurationError::PlanDigestMismatch`], [`CurationError::DecisionDigestMismatch`],
/// a batch-ordering refusal, [`CurationError::DecisionBatchFingerprintMismatch`],
/// or [`CurationError::DecisionProjectionMismatch`] (plus any refusal from
/// re-deriving the plan body).
pub fn verify_plan(plan: &DryRunPlan, manifest: &CorpusManifest) -> Result<(), Vec<CurationError>> {
    let mut errors = Vec::new();

    let recomputed_plan = plan_digest(plan);
    if recomputed_plan != plan.plan_digest {
        errors.push(CurationError::PlanDigestMismatch {
            expected: recomputed_plan,
            actual: plan.plan_digest.clone(),
        });
    }
    let recomputed_decisions = decisions_digest(&plan.decision_batch);
    if recomputed_decisions != plan.decisions_digest {
        errors.push(CurationError::DecisionDigestMismatch {
            expected: recomputed_decisions,
            actual: plan.decisions_digest.clone(),
        });
    }
    if let Err(mut e) = validate_batch(&plan.decision_batch) {
        errors.append(&mut e);
    }

    // Both the top-level corpus binding and the embedded batch's binding must
    // match the corpus — checked separately so each refusal is attributed to
    // the field that disagreed and reports *that* field's own rejected value.
    let fingerprint = corpus_fingerprint(manifest);
    if plan.input_corpus_fingerprint != fingerprint {
        errors.push(CurationError::PlanCorpusFingerprintMismatch {
            expected: fingerprint.clone(),
            actual: plan.input_corpus_fingerprint.clone(),
        });
    }
    if plan.decision_batch.input_corpus_fingerprint != fingerprint {
        errors.push(CurationError::DecisionBatchFingerprintMismatch {
            expected: fingerprint,
            actual: plan.decision_batch.input_corpus_fingerprint.clone(),
        });
    }
    // Schema / policy must be the ones this tool produces.
    if plan.schema != PLAN_SCHEMA
        || plan.policy_id != POLICY_ID
        || plan.policy_version != POLICY_VERSION
    {
        errors.push(CurationError::DecisionProjectionMismatch {
            detail: "plan schema/policy is not the canonical decision-core contract".to_owned(),
        });
    }
    // Assignments + complete songs map must be reproduced from the embedded events.
    match derive(manifest, &plan.decision_batch) {
        Ok(body) => {
            if body.assignments != plan.assignments
                || body.generated_songs_map != plan.generated_songs_map
            {
                errors.push(CurationError::DecisionProjectionMismatch {
                    detail: "re-derived projection does not match the plan's assignments/songs map"
                        .to_owned(),
                });
            }
        }
        Err(mut e) => errors.append(&mut e),
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
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
    fn verify_plan_reports_top_level_fingerprint_real_value() {
        // The embedded batch still replays correctly, but the plan's top-level
        // corpus binding is forged — the verifier must catch the contradiction,
        // attribute it to the top-level field, and report the *forged* value it
        // rejected (not the batch's, and not the expected one).
        let (m, mut plan) = good_plan();
        plan.input_corpus_fingerprint = "forged-top".to_owned();
        plan.plan_digest = plan_digest(&plan);
        let errs = verify_plan(&plan, &m).expect_err("forged top-level fingerprint");
        assert!(
            errs.iter().any(|e| matches!(
                e,
                CurationError::PlanCorpusFingerprintMismatch { actual, .. } if actual == "forged-top"
            )),
            "top-level mismatch must report its own forged value"
        );
        assert!(
            !errs
                .iter()
                .any(|e| matches!(e, CurationError::DecisionBatchFingerprintMismatch { .. })),
            "the embedded batch fingerprint is fine — no batch-level mismatch"
        );
    }

    #[test]
    fn verify_plan_reports_embedded_fingerprint_real_value() {
        // Only the embedded batch's fingerprint is forged. The refusal must be
        // attributed to the batch and must report the batch's *own* forged value
        // — the historical bug reported the (correct) top-level value instead.
        let (m, mut plan) = good_plan();
        plan.decision_batch.input_corpus_fingerprint = "forged-embedded".to_owned();
        plan.decisions_digest = decisions_digest(&plan.decision_batch);
        plan.plan_digest = plan_digest(&plan);
        let errs = verify_plan(&plan, &m).expect_err("forged embedded fingerprint");
        assert!(
            errs.iter().any(|e| matches!(
                e,
                CurationError::DecisionBatchFingerprintMismatch { actual, .. }
                    if actual == "forged-embedded"
            )),
            "embedded mismatch must report the batch's own forged value"
        );
        assert!(
            !errs
                .iter()
                .any(|e| matches!(e, CurationError::PlanCorpusFingerprintMismatch { .. })),
            "the top-level fingerprint is fine — no top-level mismatch"
        );
    }

    #[test]
    fn verify_plan_reports_both_fingerprints_forged_differently() {
        // Both fields forged to *distinct* values: two independent refusals,
        // each carrying the value that actually disagreed.
        let (m, mut plan) = good_plan();
        plan.input_corpus_fingerprint = "forged-top".to_owned();
        plan.decision_batch.input_corpus_fingerprint = "forged-embedded".to_owned();
        plan.decisions_digest = decisions_digest(&plan.decision_batch);
        plan.plan_digest = plan_digest(&plan);
        let errs = verify_plan(&plan, &m).expect_err("both fingerprints forged");
        assert!(errs.iter().any(|e| matches!(
            e,
            CurationError::PlanCorpusFingerprintMismatch { actual, .. } if actual == "forged-top"
        )));
        assert!(errs.iter().any(|e| matches!(
            e,
            CurationError::DecisionBatchFingerprintMismatch { actual, .. }
                if actual == "forged-embedded"
        )));
    }

    // ── artifact boundary: the plan must survive JSON (finding P1a) ─────────

    #[test]
    fn plan_survives_json_round_trip() {
        // ADR-0033's plan is an *immutable serialized artifact*. Verifying a
        // live Rust struct is not enough: build → serialize → deserialize →
        // verify the parsed artifact must hold.
        let (m, plan) = good_plan();
        let json = serde_json::to_string(&plan).expect("serialize");
        let parsed: DryRunPlan = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(parsed, plan, "round-trip must be faithful");
        assert_eq!(
            verify_plan(&parsed, &m),
            Ok(()),
            "a deserialized faithful plan verifies"
        );
    }

    #[test]
    fn deserialized_tampered_assignments_refuse_projection() {
        // Tamper the assignments, recompute the plan_digest so the artifact is
        // internally self-consistent, then cross the real JSON boundary. The
        // verifier must still refuse by re-derivation.
        let (m, mut plan) = good_plan();
        plan.assignments[0].song_id = "song-forged".to_owned();
        plan.plan_digest = plan_digest(&plan);
        let json = serde_json::to_string(&plan).expect("serialize");
        let parsed: DryRunPlan = serde_json::from_str(&json).expect("deserialize");
        let errs = verify_plan(&parsed, &m).expect_err("tampered assignment after round-trip");
        assert!(errs
            .iter()
            .any(|e| matches!(e, CurationError::DecisionProjectionMismatch { .. })));
    }

    #[test]
    fn deserialized_forged_fingerprint_refuses() {
        let (m, mut plan) = good_plan();
        plan.input_corpus_fingerprint = "forged-top".to_owned();
        plan.plan_digest = plan_digest(&plan);
        let json = serde_json::to_string(&plan).expect("serialize");
        let parsed: DryRunPlan = serde_json::from_str(&json).expect("deserialize");
        let errs = verify_plan(&parsed, &m).expect_err("forged fingerprint after round-trip");
        assert!(errs
            .iter()
            .any(|e| matches!(e, CurationError::PlanCorpusFingerprintMismatch { .. })));
    }

    #[test]
    fn plan_rejects_unknown_fields() {
        // A versioned artifact must not silently absorb foreign fields.
        let (_m, plan) = good_plan();
        let mut value = serde_json::to_value(&plan).expect("to value");
        value
            .as_object_mut()
            .expect("plan is a JSON object")
            .insert("rogue_field".to_owned(), json!("smuggled"));
        let json = value.to_string();
        let parsed: Result<DryRunPlan, _> = serde_json::from_str(&json);
        assert!(
            parsed.is_err(),
            "unknown top-level fields must be rejected (deny_unknown_fields)"
        );
    }

    // ── ledger identity (finding P1b) ───────────────────────────────────────

    #[test]
    fn ledger_refuses_unsupported_schema() {
        let m = uncurated();
        let b = batch_for(
            &m,
            "batch1",
            vec![event("ev0", 0, accept("g", &["shaA"], "song-1"))],
        );
        let mut ledger = ledger_of(vec![b]);
        ledger.schema = "song-curation.decisions.v99".to_owned();
        let errs = build_plan(&m, &ledger, "batch1").expect_err("unsupported ledger schema");
        assert!(errs.iter().any(|e| matches!(
            e,
            CurationError::UnsupportedDecisionsLedgerSchema { schema } if schema == "song-curation.decisions.v99"
        )));
    }

    #[test]
    fn ledger_refuses_duplicate_batch_id() {
        // Two distinct batches sharing a batch_id: the selector would otherwise
        // silently pick whichever appears first, so the application identity is
        // ambiguous. This must refuse before any selection or projection.
        let m = uncurated();
        let b1 = batch_for(
            &m,
            "dup",
            vec![event("ev0", 0, accept("g", &["shaA"], "song-1"))],
        );
        let b2 = batch_for(
            &m,
            "dup",
            vec![event("ev1", 0, accept("h", &["shaB"], "song-2"))],
        );
        let errs = build_plan(&m, &ledger_of(vec![b1, b2]), "dup").expect_err("duplicate batch id");
        assert!(errs.iter().any(|e| matches!(
            e,
            CurationError::DuplicateDecisionBatchId { batch_id } if batch_id == "dup"
        )));
    }

    // ── single-emit + short-circuit (finding P2a) ───────────────────────────

    #[test]
    fn build_plan_emits_one_ordering_error() {
        // The batch is validated once (by validate_ledger), not again in derive.
        let m = uncurated();
        let mut b = batch_for(
            &m,
            "batch1",
            vec![event("ev0", 0, accept("g", &["shaA"], "song-1"))],
        );
        b.events[0].ordinal = 7;
        let errs = build_plan(&m, &ledger_of(vec![b]), "batch1").expect_err("bad ordinal");
        let n = errs
            .iter()
            .filter(|e| matches!(e, CurationError::InvalidDecisionBatchOrder { .. }))
            .count();
        assert_eq!(n, 1, "the ordering refusal must be emitted exactly once");
    }

    #[test]
    fn verify_plan_emits_one_ordering_error() {
        let (m, mut plan) = good_plan();
        plan.decision_batch.events[0].ordinal = 4;
        plan.decisions_digest = decisions_digest(&plan.decision_batch);
        plan.plan_digest = plan_digest(&plan);
        let errs = verify_plan(&plan, &m).expect_err("bad ordinal in embedded batch");
        let n = errs
            .iter()
            .filter(|e| matches!(e, CurationError::InvalidDecisionBatchOrder { .. }))
            .count();
        assert_eq!(n, 1, "the ordering refusal must be emitted exactly once");
    }

    #[test]
    fn invalid_ledger_short_circuits_before_projection() {
        // A structurally invalid ledger (bad ordinal) that also references an
        // unknown source: refusal must come from ledger validation and stop
        // before projection, so the projection-level error never appears.
        let m = uncurated();
        let mut b = batch_for(
            &m,
            "batch1",
            vec![event("ev0", 0, accept("g", &["shaZ"], "song-1"))],
        );
        b.events[0].ordinal = 9;
        let errs = build_plan(&m, &ledger_of(vec![b]), "batch1").expect_err("invalid ledger");
        assert!(
            errs.iter()
                .any(|e| matches!(e, CurationError::InvalidDecisionBatchOrder { .. })),
            "structural refusal is reported"
        );
        assert!(
            !errs
                .iter()
                .any(|e| matches!(e, CurationError::UnknownDecisionSource { .. })),
            "projection must not run on a structurally invalid ledger"
        );
    }

    // ── strict parsing of the whole artifact graph (re-review #1) ───────────

    fn good_ledger() -> DecisionsLedger {
        let m = uncurated();
        let b = batch_for(
            &m,
            "batch1",
            vec![event("ev0", 0, accept("g", &["shaA"], "song-1"))],
        );
        ledger_of(vec![b])
    }

    /// Insert a foreign key into a JSON object reached by object-key / array-index
    /// steps, so each test can plant a rogue field at one precise depth.
    fn plant_rogue(root: &mut Value, path: &[&str]) {
        let mut cursor = root;
        for step in path {
            cursor = match step.parse::<usize>() {
                Ok(i) => cursor.get_mut(i).expect("array index exists"),
                Err(_) => cursor.get_mut(*step).expect("object key exists"),
            };
        }
        cursor
            .as_object_mut()
            .expect("target is a JSON object")
            .insert("rogue_field".to_owned(), json!("smuggled"));
    }

    #[test]
    fn ledger_rejects_unknown_field_at_root() {
        let mut v = serde_json::to_value(good_ledger()).expect("to value");
        plant_rogue(&mut v, &[]);
        assert!(serde_json::from_value::<DecisionsLedger>(v).is_err());
    }

    #[test]
    fn ledger_rejects_unknown_field_in_batch() {
        let mut v = serde_json::to_value(good_ledger()).expect("to value");
        plant_rogue(&mut v, &["batches", "0"]);
        assert!(serde_json::from_value::<DecisionsLedger>(v).is_err());
    }

    #[test]
    fn ledger_rejects_unknown_field_in_event() {
        let mut v = serde_json::to_value(good_ledger()).expect("to value");
        plant_rogue(&mut v, &["batches", "0", "events", "0"]);
        assert!(serde_json::from_value::<DecisionsLedger>(v).is_err());
    }

    #[test]
    fn ledger_rejects_unknown_field_in_action_payload() {
        // Action is internally tagged; this proves the *actual serde version*
        // denies a rogue field inside a variant payload (not just the attribute).
        let mut v = serde_json::to_value(good_ledger()).expect("to value");
        plant_rogue(&mut v, &["batches", "0", "events", "0", "action"]);
        assert!(serde_json::from_value::<DecisionsLedger>(v).is_err());
    }

    #[test]
    fn ledger_rejects_unknown_field_in_split_target() {
        let m = uncurated();
        let split = Action::Split {
            from_song_id: "song-old".to_owned(),
            into: vec![SplitTarget {
                assign_song_id: "song-1".to_owned(),
                source_sha256s: vec!["shaA".to_owned()],
            }],
            supersedes_song_ids: vec!["song-old".to_owned()],
        };
        let b = batch_for(&m, "batch1", vec![event("ev0", 0, split)]);
        let mut v = serde_json::to_value(ledger_of(vec![b])).expect("to value");
        plant_rogue(&mut v, &["batches", "0", "events", "0", "action", "into", "0"]);
        assert!(serde_json::from_value::<DecisionsLedger>(v).is_err());
    }

    #[test]
    fn plan_rejects_unknown_field_in_embedded_batch() {
        let (_m, plan) = good_plan();
        let mut v = serde_json::to_value(&plan).expect("to value");
        plant_rogue(&mut v, &["decision_batch"]);
        assert!(serde_json::from_value::<DryRunPlan>(v).is_err());
    }

    #[test]
    fn plan_rejects_unknown_field_in_assignment() {
        let (_m, plan) = good_plan();
        let mut v = serde_json::to_value(&plan).expect("to value");
        plant_rogue(&mut v, &["assignments", "0"]);
        assert!(serde_json::from_value::<DryRunPlan>(v).is_err());
    }

    // ── verify_plan short-circuits an invalid batch (re-review #2) ───────────

    #[test]
    fn verify_plan_short_circuits_on_invalid_batch() {
        // The embedded batch has a structural fault (bad ordinal) *and* a
        // projection fault (unknown source) *and* corrupted digests. A correct
        // short-circuit returns only the structural refusal — never the digest,
        // projection, or replay errors, proving they were not reached.
        let (m, mut plan) = good_plan();
        plan.decision_batch.events[0].ordinal = 9;
        plan.decision_batch.events[0].action = accept("g", &["shaZ"], "song-1");
        plan.decisions_digest = "corrupt".to_owned();
        plan.plan_digest = "corrupt".to_owned();
        let errs = verify_plan(&plan, &m).expect_err("invalid batch must refuse");
        assert!(errs
            .iter()
            .any(|e| matches!(e, CurationError::InvalidDecisionBatchOrder { .. })));
        for forbidden in [
            errs
                .iter()
                .any(|e| matches!(e, CurationError::UnknownDecisionSource { .. })),
            errs
                .iter()
                .any(|e| matches!(e, CurationError::PlanDigestMismatch { .. })),
            errs
                .iter()
                .any(|e| matches!(e, CurationError::DecisionDigestMismatch { .. })),
            errs
                .iter()
                .any(|e| matches!(e, CurationError::DecisionProjectionMismatch { .. })),
        ] {
            assert!(
                !forbidden,
                "digest, projection, and replay must not run on an invalid batch"
            );
        }
    }

    // ── one refusal per actual duplicate (re-review #3) ─────────────────────

    #[test]
    fn intra_batch_duplicate_event_id_emitted_once() {
        let m = uncurated();
        let b = batch_for(
            &m,
            "batch1",
            vec![
                event("dup", 0, accept("g", &["shaA"], "song-1")),
                event("dup", 1, accept("h", &["shaB"], "song-2")),
            ],
        );
        let errs = validate_ledger(&ledger_of(vec![b])).expect_err("duplicate event id");
        let n = errs
            .iter()
            .filter(
                |e| matches!(e, CurationError::DuplicateDecisionEventId { event_id } if event_id == "dup"),
            )
            .count();
        assert_eq!(n, 1, "an intra-batch duplicate must be emitted exactly once");
    }
}
