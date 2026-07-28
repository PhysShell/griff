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

use griff_core::corpus::{source_sha256, ChunkMeta, CorpusManifest, SourceRef, SCHEMA_VERSION};
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
    todo!("basename_of")
}

/// The stem of a filename: everything before the final `.`, or the whole name
/// when it has no extension.
fn stem_of(filename: &str) -> String {
    todo!("stem_of")
}

/// Collapse a set of matched tabs into a resolution: `Unique` when they all
/// share one digest (byte-identical duplicates are not ambiguous), `Ambiguous`
/// when digests differ, `Missing` when empty.
fn collapse(tabs: &[Tab]) -> SourceResolution {
    todo!("collapse")
}

/// Resolve a recorded `source.filename` to a source digest: exact basename
/// first, then extension-insensitive stem.
fn resolve_source(filename: &str, index: &TabIndex) -> SourceResolution {
    todo!("resolve_source")
}

/// Validate every chunk against the tab index and produce the set of backfills.
///
/// Fails closed: if *any* chunk is unresolved or conflicts, returns every error
/// and no plan, so a caller cannot write a partially migrated corpus. Records
/// whose digest is already present and correct produce no backfill (idempotent).
fn build_plan(chunks: &[ChunkMeta], index: &TabIndex) -> Result<Vec<Backfill>, Vec<MigrateError>> {
    todo!("build_plan")
}

/// Apply a validated plan in place. Sets `source.sha256`; never touches
/// `track_index`.
fn apply_plan(chunks: &mut [ChunkMeta], plan: &[Backfill]) {
    todo!("apply_plan")
}

// ── I/O orchestration ─────────────────────────────────────────────────────────

/// Recursively collect every file under `dir`, hard-failing on an unreadable
/// directory rather than silently skipping it.
fn walk(dir: &Path) -> Result<Vec<PathBuf>, String> {
    todo!("walk")
}

fn run(corpus_dir: &Path, tabs_dir: &Path, out_dir: &Path) -> Result<(), String> {
    todo!("run")
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
        assert_eq!(resolve_source("Artist - Song.gp5", &idx), SourceResolution::Missing);
    }

    #[test]
    fn resolve_ambiguous_on_distinct_bytes() {
        let idx = index_of(&[("a/Song.gp5", "aa"), ("b/Song.gp5", "bb")]);
        let SourceResolution::Ambiguous(paths) = resolve_source("Song.gp5", &idx) else {
            panic!("expected ambiguous");
        };
        assert_eq!(paths, vec!["a/Song.gp5".to_owned(), "b/Song.gp5".to_owned()]);
    }

    #[test]
    fn resolve_duplicate_bytes_is_unique() {
        // Two copies of the same file are not an ambiguity: the digest is pinned.
        let idx = index_of(&[("a/Song.gp5", "aa"), ("b/Song.gp5", "aa")]);
        assert_eq!(resolve_source("Song.gp5", &idx), SourceResolution::Unique("aa".to_owned()));
    }

    #[test]
    fn resolve_stem_fallback_when_extension_differs() {
        // Recorded as .gp5, on disk as .gpx of the same stem → matched by stem.
        let idx = index_of(&[("tabs/Song.gpx", "cc")]);
        assert_eq!(resolve_source("Song.gp5", &idx), SourceResolution::Unique("cc".to_owned()));
    }

    #[test]
    fn plan_backfills_missing_digest_and_leaves_track_index() {
        let mut chunks = vec![chunk("Artist - Song.gp5", None)];
        let idx = index_of(&[("tabs/Artist - Song.gp5", "aa")]);
        let plan = build_plan(&chunks, &idx).expect("resolvable");
        assert_eq!(plan, vec![Backfill { index: 0, sha256: "aa".to_owned() }]);
        apply_plan(&mut chunks, &plan);
        assert_eq!(chunks[0].source.sha256.as_deref(), Some("aa"));
        assert_eq!(chunks[0].source.track_index, None, "track_index must never be guessed");
    }

    #[test]
    fn plan_is_idempotent_on_matching_digest() {
        let chunks = vec![chunk("Artist - Song.gp5", Some("aa"))];
        let idx = index_of(&[("tabs/Artist - Song.gp5", "aa")]);
        let plan = build_plan(&chunks, &idx).expect("resolvable");
        assert!(plan.is_empty(), "already-correct digest produces no backfill");
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
        let mut chunks = vec![chunk("Present.gp5", None), chunk("Absent.gp5", None)];
        let idx = index_of(&[("tabs/Present.gp5", "aa")]);
        let errors = build_plan(&chunks, &idx).expect_err("missing source must refuse");
        assert_eq!(
            errors,
            vec![MigrateError::Missing {
                chunk: "test_001_p0".to_owned(),
                filename: "Absent.gp5".to_owned(),
            }]
        );
        // No partial application: the resolvable record keeps its empty digest.
        let before = chunks[0].source.sha256.clone();
        if let Err(_) = build_plan(&chunks, &idx) {
            // build_plan does not mutate; apply is never reached on Err.
        }
        assert_eq!(chunks[0].source.sha256, before);
        assert_eq!(chunks[0].source.sha256, None);
        // silence unused_mut on the vec we intentionally never apply to
        let _ = &mut chunks;
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
                Backfill { index: 0, sha256: "aa".to_owned() },
                Backfill { index: 1, sha256: "aa".to_owned() },
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
}
