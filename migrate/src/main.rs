//! `griff-corpus-migrate` — offline v8→v9 corpus migration (SHA-256 backfill).
//!
//! Isolated tool (ADR-0010 precedent, like `lab/` and the census): NOT a
//! workspace member, never built by CI. It rewrites a *copy* of the corpus into
//! a fresh output directory — never in place.
//!
//! # Scope
//!
//! This tool backfills [`SourceRef::sha256`] only — the mechanical, high-value
//! half of schema v9 that pins each chunk's source bytes and unblocks
//! source-file / fragment holdout. For every chunk it resolves
//! `source.filename` to a unique tab on disk, hashes the bytes, and records the
//! digest.
//!
//! It deliberately does **not** touch [`SourceRef::track_index`]. Recovering the
//! exact source track for a pre-v9 chunk needs a replay-based classification
//! (single-track inference / multi-track match / typed refusal), which is a
//! separate migration. Leaving `track_index` as `None` preserves the loader's
//! documented pre-v9 fallback (first note-bearing track); it is **never** guessed
//! as 0.
//!
//! # Contract
//!
//! - **Unambiguous join.** A chunk's source resolves to exactly one tab (by
//!   basename, then by stem). Tabs that are byte-identical duplicates do not
//!   count as ambiguous — their digest is determined.
//! - **Fail closed, no partial write.** If *any* chunk fails to resolve
//!   (missing / ambiguous) or conflicts with an existing digest, the whole run
//!   refuses and writes nothing.
//! - **Idempotent.** Re-running over already-backfilled records is a no-op: an
//!   existing digest that matches is left untouched.
//! - **Consistent.** Two chunks naming the same source file receive the same
//!   digest.

use griff_core::corpus::{source_sha256, ChunkMeta, CorpusManifest, SCHEMA_VERSION};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

// ── tab index ─────────────────────────────────────────────────────────────────

/// A tab file discovered under the tabs root: its path relative to that root
/// and the lowercase-hex SHA-256 of its bytes.
#[derive(Debug, Clone)]
struct Tab {
    relpath: String,
    sha256: String,
}

/// Filename→tab lookup built once from the tabs tree. A source name is resolved
/// by exact basename first, then by stem (extension-insensitive), mirroring the
/// census reconciliation.
#[derive(Debug, Default)]
struct TabIndex {
    by_basename: BTreeMap<String, Vec<Tab>>,
    by_stem: BTreeMap<String, Vec<Tab>>,
}

impl TabIndex {
    fn insert(&mut self, relpath: String, sha256: String) {
        let base = basename_of(&relpath);
        let stem = stem_of(&base);
        let tab = Tab { relpath, sha256 };
        self.by_basename.entry(base).or_default().push(tab.clone());
        self.by_stem.entry(stem).or_default().push(tab);
    }
}

/// The outcome of resolving a `source.filename` against the tab index.
#[derive(Debug, PartialEq, Eq)]
enum SourceResolution {
    /// Exactly one tab (or several byte-identical duplicates) matched; carries
    /// the digest.
    Unique(String),
    /// No tab matched by basename or stem.
    Missing,
    /// Several tabs with differing bytes matched; carries their sorted relpaths.
    Ambiguous(Vec<String>),
}

/// A single planned backfill: set chunk `index`'s source digest to `sha256`.
#[derive(Debug, PartialEq, Eq)]
struct Backfill {
    index: usize,
    sha256: String,
}

/// A reason a chunk could not be migrated. Collected across the whole batch so
/// one run reports every unresolved record, not just the first.
#[derive(Debug, PartialEq, Eq)]
enum MigrateError {
    /// `source.filename` matched no tab.
    Missing { chunk: String, filename: String },
    /// `source.filename` matched several distinct tabs (carries their relpaths).
    Ambiguous {
        chunk: String,
        filename: String,
        candidates: Vec<String>,
    },
    /// The record already carries a digest that disagrees with the tab's bytes.
    ShaConflict {
        chunk: String,
        filename: String,
        existing: String,
        computed: String,
    },
}

// ── pure contract ─────────────────────────────────────────────────────────────

/// The basename (final path component) of a `/`-separated relative path.
fn basename_of(path: &str) -> String {
    path.rsplit('/').next().unwrap_or(path).to_owned()
}

/// The stem of a filename: everything before the final `.`, or the whole name
/// when it has no extension.
fn stem_of(filename: &str) -> String {
    match filename.rsplit_once('.') {
        Some((stem, _ext)) => stem.to_owned(),
        None => filename.to_owned(),
    }
}

/// Collapse a set of matched tabs into a resolution: `Unique` when they all
/// share one digest (byte-identical duplicates are not ambiguous), `Ambiguous`
/// when digests differ, `Missing` when empty.
fn collapse(tabs: &[Tab]) -> SourceResolution {
    let Some(first) = tabs.first() else {
        return SourceResolution::Missing;
    };
    if tabs.iter().all(|tab| tab.sha256 == first.sha256) {
        return SourceResolution::Unique(first.sha256.clone());
    }
    let mut paths: Vec<String> = tabs.iter().map(|tab| tab.relpath.clone()).collect();
    paths.sort();
    SourceResolution::Ambiguous(paths)
}

/// Resolve a recorded `source.filename` to a source digest: exact basename
/// first, then extension-insensitive stem.
fn resolve_source(filename: &str, index: &TabIndex) -> SourceResolution {
    if let Some(tabs) = index.by_basename.get(filename) {
        return collapse(tabs);
    }
    if let Some(tabs) = index.by_stem.get(&stem_of(filename)) {
        return collapse(tabs);
    }
    SourceResolution::Missing
}

/// Validate every chunk against the tab index and produce the set of backfills.
///
/// Fails closed: if *any* chunk is unresolved or conflicts, returns every error
/// and no plan, so a caller cannot write a partially migrated corpus. Records
/// whose digest is already present and correct produce no backfill (idempotent).
fn build_plan(chunks: &[ChunkMeta], index: &TabIndex) -> Result<Vec<Backfill>, Vec<MigrateError>> {
    let mut plan = Vec::new();
    let mut errors = Vec::new();
    for (i, chunk) in chunks.iter().enumerate() {
        let filename = &chunk.source.filename;
        let sha = match resolve_source(filename, index) {
            SourceResolution::Unique(sha) => sha,
            SourceResolution::Missing => {
                errors.push(MigrateError::Missing {
                    chunk: chunk.id.0.clone(),
                    filename: filename.clone(),
                });
                continue;
            }
            SourceResolution::Ambiguous(candidates) => {
                errors.push(MigrateError::Ambiguous {
                    chunk: chunk.id.0.clone(),
                    filename: filename.clone(),
                    candidates,
                });
                continue;
            }
        };
        match &chunk.source.sha256 {
            // Idempotent: an already-recorded digest that agrees is left alone.
            Some(existing) if *existing == sha => {}
            Some(existing) => errors.push(MigrateError::ShaConflict {
                chunk: chunk.id.0.clone(),
                filename: filename.clone(),
                existing: existing.clone(),
                computed: sha,
            }),
            None => plan.push(Backfill {
                index: i,
                sha256: sha,
            }),
        }
    }
    if errors.is_empty() {
        Ok(plan)
    } else {
        Err(errors)
    }
}

/// Apply a validated plan in place. Sets `source.sha256`; never touches
/// `track_index`.
fn apply_plan(chunks: &mut [ChunkMeta], plan: &[Backfill]) {
    for backfill in plan {
        if let Some(chunk) = chunks.get_mut(backfill.index) {
            chunk.source.sha256 = Some(backfill.sha256.clone());
            // `track_index` intentionally untouched — recovering it is a
            // separate replay-based migration; it is never guessed here.
        }
    }
}

/// Render collected errors into one operator-facing refusal message.
fn render_errors(errors: &[MigrateError]) -> String {
    let mut lines = vec![format!(
        "refusing to migrate: {} unresolved record(s)",
        errors.len()
    )];
    for error in errors {
        lines.push(match error {
            MigrateError::Missing { chunk, filename } => {
                format!("  {chunk}: no tab for {filename:?}")
            }
            MigrateError::Ambiguous {
                chunk,
                filename,
                candidates,
            } => format!(
                "  {chunk}: {filename:?} matches {} distinct tabs: {}",
                candidates.len(),
                candidates.join(", ")
            ),
            MigrateError::ShaConflict {
                chunk,
                filename,
                existing,
                computed,
            } => format!(
                "  {chunk}: {filename:?} digest conflict: recorded {existing}, on disk {computed}"
            ),
        });
    }
    lines.join("\n")
}

// ── I/O orchestration ─────────────────────────────────────────────────────────

/// Recursively collect every file under `dir`, hard-failing on an unreadable
/// directory rather than silently skipping it.
fn walk(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut out = Vec::new();
    let entries = std::fs::read_dir(dir).map_err(|e| format!("read_dir {}: {e}", dir.display()))?;
    for entry in entries {
        let path = entry
            .map_err(|e| format!("entry under {}: {e}", dir.display()))?
            .path();
        if path.is_dir() {
            out.extend(walk(&path)?);
        } else {
            out.push(path);
        }
    }
    out.sort();
    Ok(out)
}

/// The `/`-separated path of `path` relative to `root`.
fn relpath(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Write `contents` to `out_dir/rel`, creating parent directories.
fn write_text(out_dir: &Path, rel: &str, contents: &str) -> Result<(), String> {
    let dest = out_dir.join(rel);
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir {}: {e}", parent.display()))?;
    }
    std::fs::write(&dest, contents).map_err(|e| format!("write {}: {e}", dest.display()))
}

fn run(corpus_dir: &Path, tabs_dir: &Path, out_dir: &Path) -> Result<(), String> {
    // 1. Index every tab by basename and stem, hashing its bytes once.
    let mut index = TabIndex::default();
    for path in walk(tabs_dir)? {
        let bytes = std::fs::read(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
        index.insert(relpath(tabs_dir, &path), source_sha256(&bytes));
    }

    // 2. Load the corpus: standalone chunk records and manifests.
    let mut chunk_files: Vec<(String, ChunkMeta)> = Vec::new();
    let mut manifests: Vec<(String, CorpusManifest)> = Vec::new();
    for path in walk(corpus_dir)? {
        let rel = relpath(corpus_dir, &path);
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        if name.ends_with(".chunk.json") {
            let text = std::fs::read_to_string(&path).map_err(|e| format!("read {rel}: {e}"))?;
            let chunk = serde_json::from_str(&text).map_err(|e| format!("parse {rel}: {e}"))?;
            chunk_files.push((rel, chunk));
        } else if name == "manifest.json" {
            let text = std::fs::read_to_string(&path).map_err(|e| format!("read {rel}: {e}"))?;
            let manifest = serde_json::from_str(&text).map_err(|e| format!("parse {rel}: {e}"))?;
            manifests.push((rel, manifest));
        }
    }

    // 3. Validate everything before writing anything (fail closed).
    let mut errors: Vec<MigrateError> = Vec::new();
    let mut chunk_metas: Vec<ChunkMeta> =
        chunk_files.iter().map(|(_, chunk)| chunk.clone()).collect();
    let chunk_plan = match build_plan(&chunk_metas, &index) {
        Ok(plan) => plan,
        Err(mut errs) => {
            errors.append(&mut errs);
            Vec::new()
        }
    };
    let mut manifest_plans: Vec<Vec<Backfill>> = Vec::new();
    for (_, manifest) in &manifests {
        match build_plan(&manifest.chunks, &index) {
            Ok(plan) => manifest_plans.push(plan),
            Err(mut errs) => {
                errors.append(&mut errs);
                manifest_plans.push(Vec::new());
            }
        }
    }
    if !errors.is_empty() {
        return Err(render_errors(&errors));
    }

    // 4. Apply and write. Chunk files keep their format; manifests also advance
    //    `schema_version` to the current v9.
    apply_plan(&mut chunk_metas, &chunk_plan);
    for ((rel, _), meta) in chunk_files.iter().zip(&chunk_metas) {
        let json =
            serde_json::to_string_pretty(meta).map_err(|e| format!("serialize {rel}: {e}"))?;
        write_text(out_dir, rel, &json)?;
    }
    for ((rel, manifest), plan) in manifests.iter().zip(&manifest_plans) {
        let mut migrated = manifest.clone();
        apply_plan(&mut migrated.chunks, plan);
        migrated.schema_version = SCHEMA_VERSION;
        let json =
            serde_json::to_string_pretty(&migrated).map_err(|e| format!("serialize {rel}: {e}"))?;
        write_text(out_dir, rel, &json)?;
    }

    println!(
        "migrated {} chunk file(s) + {} manifest(s) → {}",
        chunk_files.len(),
        manifests.len(),
        out_dir.display()
    );
    Ok(())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let [_, corpus, tabs, out] = args.as_slice() else {
        eprintln!("usage: migrate-v9 <corpus_dir> <tabs_dir> <out_dir>");
        return ExitCode::FAILURE;
    };
    match run(Path::new(corpus), Path::new(tabs), Path::new(out)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("migrate-v9: {e}");
            ExitCode::FAILURE
        }
    }
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal valid pre-v9 chunk (no `sha256`, no `track_index`). Tests parse
    /// it and override `source.filename` / `source.sha256` as needed.
    const V8_CHUNK: &str = r#"{
        "id": "test_001_p0",
        "title": "Test",
        "source": { "filename": "Artist - Song.gp5", "format": "gp5", "bar_range": null },
        "tempo_bpm": 120.0,
        "ticks_per_quarter": 960,
        "time_signature": [4, 4],
        "tuning": "standard_e",
        "tags": [],
        "boundaries": [],
        "techniques": [],
        "quality_flags": [],
        "created_at": "2026-01-01T00:00:00Z",
        "updated_at": "2026-01-01T00:00:00Z"
    }"#;

    fn chunk(filename: &str, sha256: Option<&str>) -> ChunkMeta {
        let mut c: ChunkMeta = serde_json::from_str(V8_CHUNK).expect("fixture parses");
        c.source.filename = filename.to_owned();
        c.source.sha256 = sha256.map(ToOwned::to_owned);
        c
    }

    fn index_of(entries: &[(&str, &str)]) -> TabIndex {
        let mut idx = TabIndex::default();
        for (relpath, sha) in entries {
            idx.insert((*relpath).to_owned(), (*sha).to_owned());
        }
        idx
    }

    #[test]
    fn stem_strips_final_extension_only() {
        assert_eq!(stem_of("Artist - Song.gp5"), "Artist - Song");
        assert_eq!(stem_of("no_ext"), "no_ext");
        assert_eq!(stem_of("a.b.gp5"), "a.b");
    }

    #[test]
    fn basename_takes_final_component() {
        assert_eq!(basename_of("dir/sub/Song.gp5"), "Song.gp5");
        assert_eq!(basename_of("Song.gp5"), "Song.gp5");
    }

    #[test]
    fn resolve_unique_by_basename() {
        let idx = index_of(&[("tabs/Artist - Song.gp5", "aa")]);
        assert_eq!(
            resolve_source("Artist - Song.gp5", &idx),
            SourceResolution::Unique("aa".to_owned())
        );
    }

    #[test]
    fn resolve_missing_when_no_tab() {
        let idx = index_of(&[("tabs/Other.gp5", "aa")]);
        assert_eq!(
            resolve_source("Artist - Song.gp5", &idx),
            SourceResolution::Missing
        );
    }

    #[test]
    fn resolve_ambiguous_on_distinct_bytes() {
        let idx = index_of(&[("a/Song.gp5", "aa"), ("b/Song.gp5", "bb")]);
        let SourceResolution::Ambiguous(paths) = resolve_source("Song.gp5", &idx) else {
            panic!("expected ambiguous");
        };
        assert_eq!(
            paths,
            vec!["a/Song.gp5".to_owned(), "b/Song.gp5".to_owned()]
        );
    }

    #[test]
    fn resolve_duplicate_bytes_is_unique() {
        // Two copies of the same file are not an ambiguity: the digest is pinned.
        let idx = index_of(&[("a/Song.gp5", "aa"), ("b/Song.gp5", "aa")]);
        assert_eq!(
            resolve_source("Song.gp5", &idx),
            SourceResolution::Unique("aa".to_owned())
        );
    }

    #[test]
    fn resolve_stem_fallback_when_extension_differs() {
        // Recorded as .gp5, on disk as .gpx of the same stem → matched by stem.
        let idx = index_of(&[("tabs/Song.gpx", "cc")]);
        assert_eq!(
            resolve_source("Song.gp5", &idx),
            SourceResolution::Unique("cc".to_owned())
        );
    }

    #[test]
    fn plan_backfills_missing_digest_and_leaves_track_index() {
        let mut chunks = vec![chunk("Artist - Song.gp5", None)];
        let idx = index_of(&[("tabs/Artist - Song.gp5", "aa")]);
        let plan = build_plan(&chunks, &idx).expect("resolvable");
        assert_eq!(
            plan,
            vec![Backfill {
                index: 0,
                sha256: "aa".to_owned()
            }]
        );
        apply_plan(&mut chunks, &plan);
        assert_eq!(chunks[0].source.sha256.as_deref(), Some("aa"));
        assert_eq!(
            chunks[0].source.track_index, None,
            "track_index must never be guessed"
        );
    }

    #[test]
    fn plan_is_idempotent_on_matching_digest() {
        let chunks = vec![chunk("Artist - Song.gp5", Some("aa"))];
        let idx = index_of(&[("tabs/Artist - Song.gp5", "aa")]);
        let plan = build_plan(&chunks, &idx).expect("resolvable");
        assert!(
            plan.is_empty(),
            "already-correct digest produces no backfill"
        );
    }

    #[test]
    fn plan_refuses_on_digest_conflict() {
        let chunks = vec![chunk("Artist - Song.gp5", Some("deadbeef"))];
        let idx = index_of(&[("tabs/Artist - Song.gp5", "aa")]);
        let errors = build_plan(&chunks, &idx).expect_err("conflict must refuse");
        assert_eq!(
            errors,
            vec![MigrateError::ShaConflict {
                chunk: "test_001_p0".to_owned(),
                filename: "Artist - Song.gp5".to_owned(),
                existing: "deadbeef".to_owned(),
                computed: "aa".to_owned(),
            }]
        );
    }

    #[test]
    fn plan_refuses_whole_batch_when_one_source_missing() {
        // One resolvable, one missing → the whole plan fails; nothing is applied.
        let chunks = vec![chunk("Present.gp5", None), chunk("Absent.gp5", None)];
        let idx = index_of(&[("tabs/Present.gp5", "aa")]);
        let errors = build_plan(&chunks, &idx).expect_err("missing source must refuse");
        assert_eq!(
            errors,
            vec![MigrateError::Missing {
                chunk: "test_001_p0".to_owned(),
                filename: "Absent.gp5".to_owned(),
            }]
        );
        // No partial migration: `build_plan` borrows immutably and returns Err
        // before any `apply_plan`, so the resolvable record keeps its empty
        // digest and nothing can leak to disk.
        assert_eq!(
            chunks.first().and_then(|c| c.source.sha256.as_deref()),
            None
        );
    }

    #[test]
    fn plan_refuses_on_ambiguous_source() {
        let chunks = vec![chunk("Song.gp5", None)];
        let idx = index_of(&[("a/Song.gp5", "aa"), ("b/Song.gp5", "bb")]);
        let errors = build_plan(&chunks, &idx).expect_err("ambiguous source must refuse");
        assert_eq!(
            errors,
            vec![MigrateError::Ambiguous {
                chunk: "test_001_p0".to_owned(),
                filename: "Song.gp5".to_owned(),
                candidates: vec!["a/Song.gp5".to_owned(), "b/Song.gp5".to_owned()],
            }]
        );
    }

    #[test]
    fn same_filename_receives_same_digest() {
        let chunks = vec![chunk("Song.gp5", None), chunk("Song.gp5", None)];
        let idx = index_of(&[("tabs/Song.gp5", "aa")]);
        let plan = build_plan(&chunks, &idx).expect("resolvable");
        assert_eq!(
            plan,
            vec![
                Backfill {
                    index: 0,
                    sha256: "aa".to_owned()
                },
                Backfill {
                    index: 1,
                    sha256: "aa".to_owned()
                },
            ]
        );
    }

    #[test]
    fn real_digest_matches_core_helper() {
        // The migration must record exactly what the loader will recompute.
        let bytes = b"guitar pro bytes";
        let sha = source_sha256(bytes);
        let idx = index_of(&[("tabs/Song.gp5", &sha)]);
        let chunks = vec![chunk("Song.gp5", None)];
        let plan = build_plan(&chunks, &idx).expect("resolvable");
        assert_eq!(plan.first().map(|b| b.sha256.as_str()), Some(sha.as_str()));
    }

    // ── end-to-end `run()` over real filesystem trees ──────────────────────────

    use std::sync::atomic::{AtomicU32, Ordering};

    /// A self-cleaning temporary directory tree for exercising `run()`.
    struct TmpTree {
        root: PathBuf,
    }

    impl TmpTree {
        fn new(tag: &str) -> Self {
            static COUNTER: AtomicU32 = AtomicU32::new(0);
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let root = std::env::temp_dir()
                .join(format!("griff-migrate-{tag}-{}-{n}", std::process::id()));
            std::fs::create_dir_all(&root).expect("create tmp root");
            Self { root }
        }

        fn path(&self, rel: &str) -> PathBuf {
            self.root.join(rel)
        }

        fn write(&self, rel: &str, contents: &str) {
            let dest = self.root.join(rel);
            if let Some(parent) = dest.parent() {
                std::fs::create_dir_all(parent).expect("create parent");
            }
            std::fs::write(&dest, contents).expect("write fixture");
        }
    }

    impl Drop for TmpTree {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn manifest_json(schema_version: u32, filename: &str) -> String {
        format!(
            r#"{{ "schema_version": {schema_version}, "chunks": [
                {{ "id": "e2e_001_p0", "title": "E2E", "source": {{ "filename": {filename:?}, "format": "gp5", "bar_range": null }},
                   "tempo_bpm": 120.0, "ticks_per_quarter": 960, "time_signature": [4, 4], "tuning": "standard_e",
                   "tags": [], "boundaries": [], "techniques": [], "quality_flags": [],
                   "created_at": "2026-01-01T00:00:00Z", "updated_at": "2026-01-01T00:00:00Z" }} ] }}"#
        )
    }

    fn chunk_json(filename: &str) -> String {
        format!(
            r#"{{ "id": "e2e_001_p0", "title": "E2E", "source": {{ "filename": {filename:?}, "format": "gp5", "bar_range": null }},
                "tempo_bpm": 120.0, "ticks_per_quarter": 960, "time_signature": [4, 4], "tuning": "standard_e",
                "tags": [], "boundaries": [], "techniques": [], "quality_flags": [],
                "created_at": "2026-01-01T00:00:00Z", "updated_at": "2026-01-01T00:00:00Z" }}"#
        )
    }

    /// Build a `corpus/` (one chunk file + one manifest) and a `tabs/` tree under
    /// `tree`, and return the corpus/tabs paths plus the tab's true digest.
    fn seed(tree: &TmpTree, schema_version: u32, filename: &str, tab_bytes: &str) -> (PathBuf, PathBuf, String) {
        tree.write("corpus/e2e_001.p0.chunk.json", &chunk_json(filename));
        tree.write("corpus/manifest.json", &manifest_json(schema_version, filename));
        tree.write(&format!("tabs/{filename}"), tab_bytes);
        (tree.path("corpus"), tree.path("tabs"), source_sha256(tab_bytes.as_bytes()))
    }

    fn out_chunk_sha(out: &Path) -> Option<String> {
        let text = std::fs::read_to_string(out.join("e2e_001.p0.chunk.json")).ok()?;
        let chunk: ChunkMeta = serde_json::from_str(&text).ok()?;
        chunk.source.sha256
    }

    #[test]
    fn e2e_success_backfills_and_advances_schema() {
        let tree = TmpTree::new("success");
        let (corpus, tabs, sha) = seed(&tree, 8, "Song.gp5", "abc bytes");
        let out = tree.path("out");
        run(&corpus, &tabs, &out).expect("migration succeeds");

        assert_eq!(out_chunk_sha(&out).as_deref(), Some(sha.as_str()));
        let chunk: ChunkMeta = serde_json::from_str(
            &std::fs::read_to_string(out.join("e2e_001.p0.chunk.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(chunk.source.track_index, None, "track_index must stay absent");
        let manifest: CorpusManifest = serde_json::from_str(
            &std::fs::read_to_string(out.join("manifest.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(manifest.schema_version, 9, "manifest must become exactly v9");
        assert_eq!(manifest.chunks.first().and_then(|c| c.source.sha256.as_deref()), Some(sha.as_str()));
    }

    #[test]
    fn e2e_idempotent_from_migrated_output() {
        let tree = TmpTree::new("idem");
        let (corpus, tabs, sha) = seed(&tree, 8, "Song.gp5", "abc bytes");
        let out = tree.path("out");
        run(&corpus, &tabs, &out).expect("first migration");
        let out2 = tree.path("out2");
        run(&out, &tabs, &out2).expect("re-migration from output");
        assert_eq!(out_chunk_sha(&out2).as_deref(), Some(sha.as_str()));
    }

    #[test]
    fn e2e_missing_source_writes_no_output() {
        let tree = TmpTree::new("missing");
        let (corpus, tabs, _) = seed(&tree, 8, "Song.gp5", "abc");
        std::fs::remove_file(tabs.join("Song.gp5")).unwrap();
        let out = tree.path("out");
        run(&corpus, &tabs, &out).expect_err("missing source must refuse");
        assert!(!out.exists(), "no output directory on refusal");
    }

    #[test]
    fn e2e_refuses_output_equal_to_corpus() {
        let tree = TmpTree::new("eq");
        let (corpus, tabs, _) = seed(&tree, 8, "Song.gp5", "abc");
        let before = std::fs::read_to_string(corpus.join("e2e_001.p0.chunk.json")).unwrap();
        run(&corpus, &tabs, &corpus).expect_err("output == corpus must refuse");
        let after = std::fs::read_to_string(corpus.join("e2e_001.p0.chunk.json")).unwrap();
        assert_eq!(before, after, "corpus must be byte-identical after refusal");
    }

    #[test]
    fn e2e_refuses_output_inside_corpus() {
        let tree = TmpTree::new("incorpus");
        let (corpus, tabs, _) = seed(&tree, 8, "Song.gp5", "abc");
        let out = corpus.join("nested_out");
        run(&corpus, &tabs, &out).expect_err("output inside corpus must refuse");
        assert!(!out.exists(), "no output on refusal");
    }

    #[test]
    fn e2e_refuses_output_inside_tabs() {
        let tree = TmpTree::new("intabs");
        let (corpus, tabs, _) = seed(&tree, 8, "Song.gp5", "abc");
        let out = tabs.join("nested_out");
        run(&corpus, &tabs, &out).expect_err("output inside tabs must refuse");
        assert!(!out.exists(), "no output on refusal");
    }

    #[test]
    fn e2e_refuses_existing_output() {
        let tree = TmpTree::new("existing");
        let (corpus, tabs, _) = seed(&tree, 8, "Song.gp5", "abc");
        let out = tree.path("out");
        std::fs::create_dir_all(&out).unwrap();
        std::fs::write(out.join("sentinel"), "keep").unwrap();
        run(&corpus, &tabs, &out).expect_err("existing output must refuse");
        assert!(out.join("sentinel").exists(), "existing output left untouched");
    }

    #[test]
    #[cfg(unix)]
    fn e2e_refuses_symlink_alias_of_corpus() {
        let tree = TmpTree::new("symlink");
        let (corpus, tabs, _) = seed(&tree, 8, "Song.gp5", "abc");
        // `alias` resolves to `corpus`; output nested under it lands inside the
        // corpus once symlinks are followed.
        let alias = tree.path("alias");
        std::os::unix::fs::symlink(&corpus, &alias).unwrap();
        let out = alias.join("nested_out");
        run(&corpus, &tabs, &out).expect_err("symlink alias into corpus must refuse");
        assert!(!corpus.join("nested_out").exists(), "nothing written into corpus");
    }

    #[test]
    fn e2e_refuses_input_schema_newer_than_target() {
        let tree = TmpTree::new("v10");
        let (corpus, tabs, _) = seed(&tree, 10, "Song.gp5", "abc");
        let out = tree.path("out");
        run(&corpus, &tabs, &out).expect_err("schema newer than v9 target must refuse");
        assert!(!out.exists(), "no output on refusal");
    }
}
