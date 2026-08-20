//! Shared test support for the Slice-2 transactional-Apply suite (ADR-0033).
//!
//! Everything here is fixture/builder machinery over the frozen Slice-1
//! public API and `griff_core` types: corpus trees on disk, serialized plan
//! artifacts, application-index files, and byte-level comparison helpers.
//! No production behaviour lives here.

#![allow(dead_code)] // each integration-test binary uses a subset

use griff_core::corpus::{
    source_sha256, ChunkId, ChunkMeta, CorpusManifest, SongId, SourceFormat, SCHEMA_VERSION,
};
use griff_song_curation::{
    build_plan, corpus_fingerprint, Action, DecisionBatch, DecisionEvent, DecisionsLedger,
    DryRunPlan, SplitTarget,
};
use serde_json::{json, Value};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

// ── temp dirs without new dependencies ─────────────────────────────────────────

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// A process-unique temporary directory, removed on drop. Uniqueness comes
/// from pid + an atomic counter, so parallel tests never collide and no
/// randomness or extra dependency is needed.
pub struct TempDir {
    pub path: PathBuf,
}

impl TempDir {
    pub fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "griff-slice2-{}-{}-{}",
            std::process::id(),
            tag,
            COUNTER.fetch_add(1, Ordering::SeqCst)
        ));
        fs::create_dir_all(&path).expect("create temp dir");
        TempDir { path }
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

// ── corpus fixtures ────────────────────────────────────────────────────────────

const V10_CHUNK: &str = r#"{
    "id": "x", "title": "T",
    "source": { "filename": "f.gp5", "format": "gp5", "bar_range": null },
    "tempo_bpm": 120.0, "ticks_per_quarter": 960, "time_signature": [4, 4],
    "tuning": "standard_e", "tags": [], "boundaries": [], "techniques": [],
    "quality_flags": [], "created_at": "2026-01-01T00:00:00Z",
    "updated_at": "2026-01-01T00:00:00Z"
}"#;

/// One corpus chunk spec: `(chunk id, sha256, existing song_id)`.
#[derive(Clone, Copy)]
pub struct Chunk<'a> {
    pub id: &'a str,
    pub sha: Option<&'a str>,
    pub song: Option<&'a str>,
}

pub fn chunk(id: &str, sha: Option<&str>, song: Option<&str>) -> ChunkMeta {
    let mut c: ChunkMeta = serde_json::from_str(V10_CHUNK).expect("fixture parses");
    c.id = ChunkId(id.to_owned());
    c.title = format!("Title {id}");
    c.source.filename = format!("{id}.gp5");
    c.source.format = SourceFormat::Gp5;
    c.source.sha256 = sha.map(ToOwned::to_owned);
    c.source.song_id = song.map(|s| SongId(s.to_owned()));
    c
}

pub fn manifest_of(
    chunks: &[Chunk<'_>],
    songs: Option<BTreeMap<SongId, Vec<String>>>,
) -> CorpusManifest {
    CorpusManifest {
        schema_version: SCHEMA_VERSION,
        chunks: chunks.iter().map(|c| chunk(c.id, c.sha, c.song)).collect(),
        groups: Vec::new(),
        songs,
    }
}

/// Write a corpus snapshot tree: one `<id>.chunk.json` per chunk plus the
/// root `manifest.json`, all pretty-printed (the corpus rendering convention).
pub fn write_corpus(dir: &Path, chunks: &[Chunk<'_>]) -> CorpusManifest {
    write_corpus_with_songs(dir, chunks, None)
}

pub fn write_corpus_with_songs(
    dir: &Path,
    chunks: &[Chunk<'_>],
    songs: Option<BTreeMap<SongId, Vec<String>>>,
) -> CorpusManifest {
    fs::create_dir_all(dir).expect("create corpus dir");
    let manifest = manifest_of(chunks, songs);
    for meta in &manifest.chunks {
        let text = serde_json::to_string_pretty(meta).expect("serialize chunk");
        fs::write(dir.join(format!("{}.chunk.json", meta.id.0)), text).expect("write chunk");
    }
    let text = serde_json::to_string_pretty(&manifest).expect("serialize manifest");
    fs::write(dir.join("manifest.json"), text).expect("write manifest");
    manifest
}

pub fn read_manifest(dir: &Path) -> CorpusManifest {
    let text = fs::read_to_string(dir.join("manifest.json")).expect("read manifest");
    serde_json::from_str(&text).expect("parse manifest")
}

// ── decision events / batches / plans ──────────────────────────────────────────

pub fn event(id: &str, ordinal: u64, action: Action) -> DecisionEvent {
    DecisionEvent {
        event_id: id.to_owned(),
        ordinal,
        curator: "curator".to_owned(),
        occurred_at: "2026-08-20T00:00:00Z".to_owned(),
        note: None,
        action,
    }
}

pub fn accept(candidate: &str, shas: &[&str], song: &str, supersedes: &[&str]) -> Action {
    Action::AcceptSuggestion {
        candidate_id: candidate.to_owned(),
        source_sha256s: shas.iter().map(|s| (*s).to_owned()).collect(),
        assign_song_id: song.to_owned(),
        supersedes_song_ids: supersedes.iter().map(|s| (*s).to_owned()).collect(),
    }
}

pub fn reject(candidate: &str, shas: &[&str]) -> Action {
    Action::RejectSuggestion {
        candidate_id: candidate.to_owned(),
        reviewed_source_sha256s: shas.iter().map(|s| (*s).to_owned()).collect(),
        reason: None,
    }
}

pub fn manual(shas: &[&str], song: &str) -> Action {
    Action::ManualDefine {
        source_sha256s: shas.iter().map(|s| (*s).to_owned()).collect(),
        assign_song_id: song.to_owned(),
    }
}

pub fn correct(shas: &[&str], new: &str, supersedes: &[&str]) -> Action {
    Action::Correct {
        source_sha256s: shas.iter().map(|s| (*s).to_owned()).collect(),
        new_song_id: new.to_owned(),
        supersedes_song_ids: supersedes.iter().map(|s| (*s).to_owned()).collect(),
    }
}

pub fn merge(from: &[&str], into: &str, shas: &[&str], supersedes: &[&str]) -> Action {
    Action::Merge {
        from_song_ids: from.iter().map(|s| (*s).to_owned()).collect(),
        into_song_id: into.to_owned(),
        source_sha256s: shas.iter().map(|s| (*s).to_owned()).collect(),
        supersedes_song_ids: supersedes.iter().map(|s| (*s).to_owned()).collect(),
    }
}

pub fn split(from: &str, into: &[(&str, &[&str])], supersedes: &[&str]) -> Action {
    Action::Split {
        from_song_id: from.to_owned(),
        into: into
            .iter()
            .map(|(song, shas)| SplitTarget {
                assign_song_id: (*song).to_owned(),
                source_sha256s: shas.iter().map(|s| (*s).to_owned()).collect(),
            })
            .collect(),
        supersedes_song_ids: supersedes.iter().map(|s| (*s).to_owned()).collect(),
    }
}

pub fn batch_for(
    manifest: &CorpusManifest,
    id: &str,
    prev_report_digest: Option<&str>,
    events: Vec<DecisionEvent>,
) -> DecisionBatch {
    DecisionBatch {
        batch_id: id.to_owned(),
        input_corpus_fingerprint: corpus_fingerprint(manifest),
        previous_application_report_digest: prev_report_digest.map(ToOwned::to_owned),
        events,
    }
}

pub fn ledger_of(batches: Vec<DecisionBatch>) -> DecisionsLedger {
    DecisionsLedger {
        schema: "song-curation.decisions.v1".to_owned(),
        next_song_seq: 1,
        batches,
    }
}

/// Build a Slice-1 plan for `batch` against the corpus at `corpus_dir` and
/// serialize it to `plan_path` (the artifact boundary Apply consumes).
pub fn write_plan(corpus_dir: &Path, batch: DecisionBatch, plan_path: &Path) -> DryRunPlan {
    let manifest = read_manifest(corpus_dir);
    let id = batch.batch_id.clone();
    let plan = build_plan(&manifest, &ledger_of(vec![batch]), &id).expect("plan builds");
    fs::write(
        plan_path,
        serde_json::to_string_pretty(&plan).expect("serialize plan"),
    )
    .expect("write plan");
    plan
}

/// Load the plan artifact, mutate its JSON value, and write it back.
pub fn tamper_plan(plan_path: &Path, f: impl FnOnce(&mut Value)) {
    let text = fs::read_to_string(plan_path).expect("read plan");
    let mut value: Value = serde_json::from_str(&text).expect("parse plan");
    f(&mut value);
    fs::write(plan_path, value.to_string()).expect("write plan");
}

// ── application index files ────────────────────────────────────────────────────

pub const EMPTY_INDEX: &str =
    "{\n  \"schema\": \"song-curation.applications.v1\",\n  \"applications\": []\n}";

pub fn write_empty_index(path: &Path) {
    fs::write(path, EMPTY_INDEX).expect("write index");
}

// ── filesystem comparison helpers ──────────────────────────────────────────────

/// Every file under `dir` as `(relative path, bytes)`, sorted by path.
pub fn walk_bytes(dir: &Path) -> Vec<(String, Vec<u8>)> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, Vec<u8>)>) {
        for entry in fs::read_dir(dir).expect("read_dir") {
            let path = entry.expect("entry").path();
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                let rel = path
                    .strip_prefix(root)
                    .expect("under root")
                    .to_string_lossy()
                    .replace('\\', "/");
                out.push((rel, fs::read(&path).expect("read file")));
            }
        }
    }
    let mut out = Vec::new();
    walk(dir, dir, &mut out);
    out.sort();
    out
}

pub fn read_value(path: &Path) -> Value {
    serde_json::from_str(&fs::read_to_string(path).expect("read file")).expect("parse json")
}

pub fn sha256_of_file(path: &Path) -> String {
    source_sha256(&fs::read(path).expect("read file"))
}

/// The staging sibling the contract derives for `output` (§8.1).
pub fn staging_path_of(output: &Path) -> PathBuf {
    let name = output.file_name().expect("output name").to_string_lossy();
    output
        .parent()
        .expect("output parent")
        .join(format!(".{name}.apply-staging"))
}

/// The lock / temp coordination names for a (supplied) index path (§8.1).
pub fn lock_path_of(index: &Path) -> PathBuf {
    coordination_path(index, "lock")
}

pub fn temp_path_of(index: &Path) -> PathBuf {
    coordination_path(index, "tmp")
}

fn coordination_path(index: &Path, ext: &str) -> PathBuf {
    let name = index.file_name().expect("index name").to_string_lossy();
    index
        .parent()
        .expect("index parent")
        .join(format!(".{name}.{ext}"))
}

/// Standard four-path apply layout inside one temp root:
/// `corpus/`, `plan.json`, `index.json`, `out`.
pub struct Layout {
    pub td: TempDir,
    pub corpus: PathBuf,
    pub plan: PathBuf,
    pub index: PathBuf,
    pub output: PathBuf,
}

pub fn layout(tag: &str) -> Layout {
    let td = TempDir::new(tag);
    let corpus = td.path.join("corpus");
    let plan = td.path.join("plan.json");
    let index = td.path.join("index.json");
    let output = td.path.join("out");
    Layout {
        td,
        corpus,
        plan,
        index,
        output,
    }
}

/// Inject a rogue member into a JSON object located by `path` steps.
pub fn plant_rogue(root: &mut Value, path: &[&str]) {
    let mut cursor = root;
    for step in path {
        cursor = match step.parse::<usize>() {
            Ok(i) => cursor.get_mut(i).expect("array index exists"),
            Err(_) => cursor.get_mut(*step).expect("object key exists"),
        };
    }
    cursor
        .as_object_mut()
        .expect("target is object")
        .insert("rogue_field".to_owned(), json!("smuggled"));
}
