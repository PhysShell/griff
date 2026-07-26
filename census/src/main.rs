//! Real-corpus census — a read-only harness that reconciles the corpus records
//! against the source tabs they cite and reports migration-readiness (v8/v9/v10
//! coverage), annotation coverage, ensemble integrity, duplicates, and the
//! multi-transcription "work" candidates a `song_id` (ADR-0031) must group.
//!
//! The population is the **individual `*.chunk.json` files** (what the loader
//! globs), parsed with the production `ChunkMeta` — not a single manifest, since
//! the real corpus carries several (a small curated top-level set and a large
//! ingest set). Every `manifest.json` is loaded too, for schema version, group
//! definitions, and manifest↔file reconciliation. Tabs are hashed with the
//! production `source_sha256`, so the numbers match the future v9 backfill by
//! construction. Changes no corpus data; a directory it cannot read is a hard
//! error (never a silent partial report).
//!
//! Usage (from the repo root — `census` is excluded from the workspace, so use
//! `--manifest-path`, not `-p`):
//!   cargo run --manifest-path census/Cargo.toml -- <corpus_dir> <tabs_dir> [out_json] [out_md]

#![allow(clippy::too_many_lines, clippy::cast_precision_loss)]

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use serde::Serialize;

use griff_core::corpus::{source_sha256, ChunkMeta, CorpusManifest, EnsembleGroup, SourceFormat};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let (Some(corpus_dir), Some(tabs_dir)) = (args.get(1), args.get(2)) else {
        eprintln!("usage: census <corpus_dir> <tabs_dir> [out_json] [out_md]");
        return ExitCode::FAILURE;
    };
    let out_json = args
        .get(3)
        .cloned()
        .unwrap_or_else(|| "census/corpus-health.json".to_owned());
    let out_md = args
        .get(4)
        .cloned()
        .unwrap_or_else(|| "docs/audit/2026-07-real-corpus-health.md".to_owned());

    let health = match census(Path::new(corpus_dir), Path::new(tabs_dir)) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("census failed: {e}");
            return ExitCode::FAILURE;
        }
    };

    let json = serde_json::to_string_pretty(&health).expect("serialize");
    if let Err(e) = fs::write(&out_json, format!("{json}\n")) {
        eprintln!("write {out_json}: {e}");
        return ExitCode::FAILURE;
    }
    if let Err(e) = fs::write(&out_md, render_markdown(&health)) {
        eprintln!("write {out_md}: {e}");
        return ExitCode::FAILURE;
    }
    eprintln!("wrote {out_json} and {out_md}");
    ExitCode::SUCCESS
}

// ── report shape ──────────────────────────────────────────────────────────────

#[derive(Serialize)]
struct Health {
    /// Ties the report to an exact corpus version: SHA-256 over the sorted
    /// per-file content hashes. Deterministic re-runs on the same files match.
    corpus_digest: String,
    manifests: Vec<ManifestSummary>,
    counts: Counts,
    manifest_reconciliation: ManifestReconciliation,
    source_reconciliation: SourceReconciliation,
    v9_coverage: V9Coverage,
    v10_readiness: V10Readiness,
    annotation_coverage: AnnotationCoverage,
    ensemble_integrity: EnsembleIntegrity,
    duplicates: Duplicates,
    format_distribution: BTreeMap<String, usize>,
}

#[derive(Serialize)]
struct ManifestSummary {
    path: String,
    schema_version: u32,
    chunks: usize,
    groups: usize,
}

#[derive(Serialize)]
struct Counts {
    chunk_files: usize,
    chunk_parse_errors: usize,
    manifest_chunk_records: usize,
    unique_referenced_filenames: usize,
    tab_files: usize,
}

#[derive(Serialize)]
struct ManifestReconciliation {
    files_covered_by_a_manifest: usize,
    files_not_in_any_manifest: usize,
    manifest_ids_without_a_file: usize,
}

#[derive(Serialize)]
struct SourceReconciliation {
    matched_sources: usize,
    /// Every unique source filename the chunks reference (the full list).
    referenced_sources: Vec<String>,
    missing_sources: Vec<String>,
    ambiguous_sources: Vec<String>,
    /// Relative paths of tab files no chunk references (physical files, so tabs
    /// with a shared basename in different directories are listed separately).
    unreferenced_tabs: Vec<String>,
}

#[derive(Serialize)]
struct V9Coverage {
    sha256_present: usize,
    track_index_present: usize,
    total_chunks: usize,
}

#[derive(Serialize)]
struct V10Readiness {
    song_id_field_in_schema: bool,
    distinct_works_estimated: usize,
    multi_transcription_works: Vec<WorkGroup>,
}

#[derive(Serialize)]
struct WorkGroup {
    work: String,
    files: Vec<String>,
}

#[derive(Serialize)]
struct AnnotationCoverage {
    structure: usize,
    gesture: usize,
    complexity: usize,
    rights: usize,
    style_cohort: usize,
    duplicate: usize,
    ensemble: usize,
    total_chunks: usize,
}

#[derive(Serialize)]
struct EnsembleIntegrity {
    manifest_groups: usize,
    group_member_ids: usize,
    chunks_with_ensemble_ref: usize,
    broken_member_refs: Vec<String>,
    orphan_ensemble_refs: Vec<String>,
}

#[derive(Serialize)]
struct Duplicates {
    content_duplicate_tabs: Vec<Vec<String>>,
}

// ── census ────────────────────────────────────────────────────────────────────

fn census(corpus_dir: &Path, tabs_dir: &Path) -> Result<Health, String> {
    // 1. Population: every individual *.chunk.json, parsed with the production type.
    let mut chunks: Vec<ChunkMeta> = Vec::new();
    let mut parse_errors = 0usize;
    let mut file_hashes: Vec<String> = Vec::new();
    let mut chunk_files = 0usize;
    for path in walk(corpus_dir)? {
        if !path.to_string_lossy().ends_with(".chunk.json") {
            continue;
        }
        chunk_files += 1;
        let bytes = fs::read(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
        file_hashes.push(source_sha256(&bytes));
        match serde_json::from_slice::<ChunkMeta>(&bytes) {
            Ok(c) => chunks.push(c),
            Err(_) => parse_errors += 1,
        }
    }
    file_hashes.sort();
    let corpus_digest = source_sha256(file_hashes.join("\n").as_bytes());

    // 2. Manifests: schema version, group defs, and the ids they enumerate.
    let mut manifests: Vec<ManifestSummary> = Vec::new();
    let mut groups: Vec<EnsembleGroup> = Vec::new();
    let mut manifest_ids: BTreeSet<String> = BTreeSet::new();
    let mut manifest_chunk_records = 0usize;
    for path in walk(corpus_dir)? {
        if path.file_name().and_then(|n| n.to_str()) != Some("manifest.json") {
            continue;
        }
        let bytes = fs::read(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
        let m: CorpusManifest =
            serde_json::from_slice(&bytes).map_err(|e| format!("parse {}: {e}", path.display()))?;
        manifest_chunk_records += m.chunks.len();
        for c in &m.chunks {
            manifest_ids.insert(c.id.0.clone());
        }
        manifests.push(ManifestSummary {
            path: rel(corpus_dir, &path),
            schema_version: m.schema_version,
            chunks: m.chunks.len(),
            groups: m.groups.len(),
        });
        groups.extend(m.groups);
    }
    manifests.sort_by(|a, b| a.path.cmp(&b.path));

    // 3. Tabs: keep every physical file (relative path), indexed by basename/stem
    //    for lookup and by content hash for duplicate detection.
    let mut by_basename: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut by_stem: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut hash_to_paths: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut tab_paths: Vec<String> = Vec::new();
    for path in walk(tabs_dir)? {
        if !is_tab(&path) {
            continue;
        }
        let relpath = rel(tabs_dir, &path);
        tab_paths.push(relpath.clone());
        let name = basename(&path);
        by_basename
            .entry(name.clone())
            .or_default()
            .push(relpath.clone());
        by_stem
            .entry(stem(&name))
            .or_default()
            .push(relpath.clone());
        let bytes = fs::read(&path).map_err(|e| format!("read {}: {e}", path.display()))?;
        hash_to_paths
            .entry(source_sha256(&bytes))
            .or_default()
            .insert(relpath);
    }
    let tab_files = tab_paths.len();

    // 4. Walk the population.
    let total = chunks.len();
    let mut referenced: BTreeSet<String> = BTreeSet::new();
    let (mut sha_present, mut track_present) = (0usize, 0usize);
    let mut ann = [0usize; 7];
    let mut fmt: BTreeMap<String, usize> = BTreeMap::new();
    let mut chunk_ids: BTreeSet<String> = BTreeSet::new();
    let mut ensemble_ref_groups: BTreeSet<String> = BTreeSet::new();
    let mut chunks_with_ensemble = 0usize;
    for c in &chunks {
        chunk_ids.insert(c.id.0.clone());
        referenced.insert(c.source.filename.clone());
        sha_present += usize::from(c.source.sha256.is_some());
        track_present += usize::from(c.source.track_index.is_some());
        ann[0] += usize::from(c.structure.is_some());
        ann[1] += usize::from(c.gesture.is_some());
        ann[2] += usize::from(c.complexity.is_some());
        ann[3] += usize::from(c.rights.is_some());
        ann[4] += usize::from(c.style_cohort.is_some());
        ann[5] += usize::from(c.duplicate.is_some());
        if let Some(e) = &c.ensemble {
            ann[6] += 1;
            chunks_with_ensemble += 1;
            ensemble_ref_groups.insert(e.group_id.clone());
        }
        *fmt.entry(format_name(c.source.format)).or_default() += 1;
    }

    // 5. Manifest ↔ file reconciliation.
    let files_covered = chunk_ids
        .iter()
        .filter(|id| manifest_ids.contains(*id))
        .count();
    let files_uncovered = chunk_ids.len() - files_covered;
    let manifest_orphans = manifest_ids
        .iter()
        .filter(|id| !chunk_ids.contains(*id))
        .count();

    // 6. Source ↔ tab reconciliation (over physical tab paths).
    let (mut matched, mut missing, mut ambiguous) = (0usize, Vec::new(), Vec::new());
    for f in &referenced {
        match resolve(f, &by_basename, &by_stem) {
            1 => matched += 1,
            0 => missing.push(f.clone()),
            _ => ambiguous.push(f.clone()),
        }
    }
    let mut unreferenced_tabs: Vec<String> = tab_paths
        .iter()
        .filter(|p| !referenced_hits(&basename(Path::new(p)), &referenced))
        .cloned()
        .collect();
    unreferenced_tabs.sort();
    unreferenced_tabs.dedup();
    missing.sort();
    ambiguous.sort();

    // 7. Works — normalized composition key (strips the `(ver N by X)` suffix).
    let mut works: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for f in &referenced {
        works.entry(work_key(f)).or_default().insert(f.clone());
    }
    let mut multi: Vec<WorkGroup> = works
        .iter()
        .filter(|(_, files)| files.len() > 1)
        .map(|(w, files)| WorkGroup {
            work: w.clone(),
            files: files.iter().cloned().collect(),
        })
        .collect();
    multi.sort_by(|a, b| b.files.len().cmp(&a.files.len()).then(a.work.cmp(&b.work)));

    // 8. Ensemble integrity (union of all manifests' groups vs the population).
    let mut group_ids: BTreeSet<String> = BTreeSet::new();
    let mut group_member_ids = 0usize;
    let mut broken = Vec::new();
    for g in &groups {
        group_ids.insert(g.id.clone());
        for m in &g.members {
            group_member_ids += 1;
            if !chunk_ids.contains(&m.0) {
                broken.push(format!("group {} -> missing chunk {}", g.id, m.0));
            }
        }
    }
    let mut orphan: Vec<String> = ensemble_ref_groups
        .iter()
        .filter(|g| !group_ids.contains(*g))
        .cloned()
        .collect();
    orphan.sort();
    broken.sort();
    broken.truncate(50);

    // 9. Content-duplicate tabs.
    let mut content_dupes: Vec<Vec<String>> = hash_to_paths
        .values()
        .filter(|paths| paths.len() > 1)
        .map(|paths| paths.iter().cloned().collect())
        .collect();
    content_dupes.sort();

    Ok(Health {
        corpus_digest,
        manifests,
        counts: Counts {
            chunk_files,
            chunk_parse_errors: parse_errors,
            manifest_chunk_records,
            unique_referenced_filenames: referenced.len(),
            tab_files,
        },
        manifest_reconciliation: ManifestReconciliation {
            files_covered_by_a_manifest: files_covered,
            files_not_in_any_manifest: files_uncovered,
            manifest_ids_without_a_file: manifest_orphans,
        },
        source_reconciliation: SourceReconciliation {
            matched_sources: matched,
            referenced_sources: referenced.iter().cloned().collect(),
            missing_sources: missing,
            ambiguous_sources: ambiguous,
            unreferenced_tabs,
        },
        v9_coverage: V9Coverage {
            sha256_present: sha_present,
            track_index_present: track_present,
            total_chunks: total,
        },
        v10_readiness: V10Readiness {
            song_id_field_in_schema: false,
            distinct_works_estimated: works.len(),
            multi_transcription_works: multi,
        },
        annotation_coverage: AnnotationCoverage {
            structure: ann[0],
            gesture: ann[1],
            complexity: ann[2],
            rights: ann[3],
            style_cohort: ann[4],
            duplicate: ann[5],
            ensemble: ann[6],
            total_chunks: total,
        },
        ensemble_integrity: EnsembleIntegrity {
            manifest_groups: groups.len(),
            group_member_ids,
            chunks_with_ensemble_ref: chunks_with_ensemble,
            broken_member_refs: broken,
            orphan_ensemble_refs: orphan,
        },
        duplicates: Duplicates {
            content_duplicate_tabs: content_dupes,
        },
        format_distribution: fmt,
    })
}

// ── helpers ───────────────────────────────────────────────────────────────────

/// All files under `dir`, recursively, sorted. A directory that cannot be read
/// is a hard error, so the census never silently reports over a partial corpus.
fn walk(dir: &Path) -> Result<Vec<PathBuf>, String> {
    let mut out = Vec::new();
    let entries = fs::read_dir(dir).map_err(|e| format!("read_dir {}: {e}", dir.display()))?;
    for entry in entries {
        let path = entry
            .map_err(|e| format!("dir entry in {}: {e}", dir.display()))?
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

fn rel(base: &Path, p: &Path) -> String {
    p.strip_prefix(base)
        .unwrap_or(p)
        .to_string_lossy()
        .into_owned()
}

fn is_tab(p: &Path) -> bool {
    matches!(
        p.extension().and_then(|e| e.to_str()),
        Some("gp" | "gp3" | "gp4" | "gp5" | "gpx" | "mid" | "midi")
    )
}

fn basename(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn stem(name: &str) -> String {
    name.rsplit_once('.')
        .map_or_else(|| name.to_owned(), |(s, _)| s.to_owned())
}

fn resolve(
    name: &str,
    by_basename: &BTreeMap<String, Vec<String>>,
    by_stem: &BTreeMap<String, Vec<String>>,
) -> usize {
    if let Some(v) = by_basename.get(name) {
        return v.len();
    }
    by_stem.get(&stem(name)).map_or(0, Vec::len)
}

fn referenced_hits(tab_name: &str, referenced: &BTreeSet<String>) -> bool {
    referenced.contains(tab_name) || referenced.iter().any(|r| stem(r) == stem(tab_name))
}

/// Strip a trailing parenthesized transcriber/version suffix — the real corpus
/// naming is `Artist - Title (ver N by transcriber).ext`. Only a paren group
/// that starts with `ver`, contains ` by `, or is all digits is stripped, so a
/// title that legitimately ends in parentheses (e.g. `(Reprise)`) is preserved.
fn strip_version_suffix(s: &str) -> &str {
    let trimmed = s.trim_end();
    if !trimmed.ends_with(')') {
        return s;
    }
    let Some(open) = trimmed.rfind('(') else {
        return s;
    };
    let inner = trimmed[open + 1..trimmed.len() - 1].trim();
    let lower = inner.to_lowercase();
    let is_version = lower.starts_with("ver")
        || lower.contains(" by ")
        || (!inner.is_empty() && inner.chars().all(|c| c.is_ascii_digit()));
    if is_version {
        trimmed[..open].trim_end()
    } else {
        s
    }
}

/// Normalize a source filename to a composition (Work) key: drop the extension
/// and the `(ver N by X)` suffix, lowercase, collapse whitespace.
fn work_key(filename: &str) -> String {
    let base = strip_version_suffix(&stem(filename)).to_lowercase();
    base.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn format_name(f: SourceFormat) -> String {
    match f {
        SourceFormat::Midi => "midi",
        SourceFormat::Gp3 => "gp3",
        SourceFormat::Gp4 => "gp4",
        SourceFormat::Gp5 => "gp5",
        SourceFormat::Gpx => "gpx",
        SourceFormat::Gp => "gp",
    }
    .to_owned()
}

// ── markdown (rendered from the same struct — one source of truth) ────────────

fn pct(n: usize, total: usize) -> String {
    if total == 0 {
        "n/a".to_owned()
    } else {
        format!("{:.1}%", 100.0 * n as f64 / total as f64)
    }
}

fn render_markdown(h: &Health) -> String {
    let total = h.counts.chunk_files;
    let a = &h.annotation_coverage;
    let sr = &h.source_reconciliation;
    let mut s = String::new();
    s.push_str("# 2026-07 — Real corpus health census\n\n");
    s.push_str(
        "Read-only census of the actual corpus, generated by the isolated \
         `census/` tool (`cargo run --manifest-path census/Cargo.toml -- \
         <corpus_dir> <tabs_dir>`). Rendered from `corpus-health.json`; \
         re-running on the same files reproduces both. Changes no corpus data. \
         First artifact to ground the contract series (benchmark, reachability \
         lab, ADR-0031 song identity) against the real corpus rather than \
         synthetic data.\n\n",
    );
    s.push_str(&format!(
        "Corpus digest (SHA-256 over sorted per-file hashes): `{}`\n\n",
        h.corpus_digest
    ));

    s.push_str("## Layers of the corpus\n\n");
    s.push_str(&format!(
        "The population is the **{} individual `*.chunk.json` files** (what the \
         loader globs); {} failed to parse. Separately, {} `manifest.json` file(s) \
         enumerate their own subsets:\n\n",
        h.counts.chunk_files,
        h.counts.chunk_parse_errors,
        h.manifests.len()
    ));
    s.push_str("| Manifest | schema | chunks | groups |\n| --- | --- | --- | --- |\n");
    for m in &h.manifests {
        s.push_str(&format!(
            "| `{}` | {} | {} | {} |\n",
            m.path, m.schema_version, m.chunks, m.groups
        ));
    }
    let mr = &h.manifest_reconciliation;
    s.push_str(&format!(
        "\nManifest ↔ file reconciliation: **{}** files covered by a manifest, \
         **{}** files in no manifest, **{}** manifest ids with no file.\n\n",
        mr.files_covered_by_a_manifest,
        mr.files_not_in_any_manifest,
        mr.manifest_ids_without_a_file
    ));

    s.push_str("## Counts\n\n| Fact | Value |\n| --- | --- |\n");
    s.push_str(&format!(
        "| chunk files (population) | **{}** |\n",
        h.counts.chunk_files
    ));
    s.push_str(&format!(
        "| chunk parse errors | {} |\n",
        h.counts.chunk_parse_errors
    ));
    s.push_str(&format!(
        "| manifest chunk records (all manifests) | {} |\n",
        h.counts.manifest_chunk_records
    ));
    s.push_str(&format!(
        "| unique referenced source filenames | {} |\n",
        h.counts.unique_referenced_filenames
    ));
    s.push_str(&format!(
        "| tab files under `tabs/` | {} |\n\n",
        h.counts.tab_files
    ));

    s.push_str("## Source ↔ tab reconciliation\n\n| Outcome | Count |\n| --- | --- |\n");
    s.push_str(&format!("| matched sources | {} |\n", sr.matched_sources));
    s.push_str(&format!(
        "| missing (referenced, no tab) | {} |\n",
        sr.missing_sources.len()
    ));
    s.push_str(&format!(
        "| ambiguous (referenced → >1 tab) | {} |\n",
        sr.ambiguous_sources.len()
    ));
    s.push_str(&format!(
        "| unreferenced tabs | {} |\n\n",
        sr.unreferenced_tabs.len()
    ));
    list_section(&mut s, "Referenced sources", &sr.referenced_sources, 60);
    list_section(&mut s, "Missing sources", &sr.missing_sources, 40);
    list_section(&mut s, "Ambiguous sources", &sr.ambiguous_sources, 40);
    list_section(&mut s, "Unreferenced tabs", &sr.unreferenced_tabs, 60);

    s.push_str("## Migration readiness\n\n");
    s.push_str(
        "**v9 (source-file identity).**\n\n| Field | Present | Coverage |\n| --- | --- | --- |\n",
    );
    s.push_str(&format!(
        "| `sha256` | {} | {} |\n| `track_index` | {} | {} |\n\n",
        h.v9_coverage.sha256_present,
        pct(h.v9_coverage.sha256_present, total),
        h.v9_coverage.track_index_present,
        pct(h.v9_coverage.track_index_present, total),
    ));
    s.push_str(&format!(
        "**v10 (song identity).** `song_id` field in schema: **{}**. Estimated \
         distinct compositions (Work candidates): **{}**. Multi-transcription works \
         (the leakage ADR-0031 closes): **{}** (covering {} source files).\n\n",
        h.v10_readiness.song_id_field_in_schema,
        h.v10_readiness.distinct_works_estimated,
        h.v10_readiness.multi_transcription_works.len(),
        h.v10_readiness
            .multi_transcription_works
            .iter()
            .map(|w| w.files.len())
            .sum::<usize>(),
    ));
    if !h.v10_readiness.multi_transcription_works.is_empty() {
        s.push_str("<details><summary>Multi-transcription works</summary>\n\n");
        s.push_str("| Work (normalized) | Source files |\n| --- | --- |\n");
        for w in &h.v10_readiness.multi_transcription_works {
            s.push_str(&format!("| `{}` | {} |\n", w.work, w.files.join("<br>")));
        }
        s.push_str("\n</details>\n\n");
    }

    s.push_str("## Annotation coverage\n\n| Field | Present | Coverage |\n| --- | --- | --- |\n");
    for (name, n) in [
        ("structure", a.structure),
        ("gesture", a.gesture),
        ("complexity", a.complexity),
        ("rights", a.rights),
        ("style_cohort", a.style_cohort),
        ("duplicate", a.duplicate),
        ("ensemble", a.ensemble),
    ] {
        s.push_str(&format!("| `{name}` | {n} | {} |\n", pct(n, total)));
    }
    s.push('\n');

    s.push_str("## Ensemble integrity\n\n| Fact | Value |\n| --- | --- |\n");
    let e = &h.ensemble_integrity;
    s.push_str(&format!(
        "| groups (all manifests) | {} |\n",
        e.manifest_groups
    ));
    s.push_str(&format!("| group member ids | {} |\n", e.group_member_ids));
    s.push_str(&format!(
        "| chunks with ensemble ref | {} |\n",
        e.chunks_with_ensemble_ref
    ));
    s.push_str(&format!(
        "| broken member refs | {} |\n",
        e.broken_member_refs.len()
    ));
    s.push_str(&format!(
        "| orphan ensemble refs | {} |\n\n",
        e.orphan_ensemble_refs.len()
    ));
    list_section(&mut s, "Broken member refs", &e.broken_member_refs, 40);
    list_section(&mut s, "Orphan ensemble refs", &e.orphan_ensemble_refs, 40);

    s.push_str("## Duplicates & formats\n\n");
    s.push_str(&format!(
        "Content-duplicate tab groups (identical bytes, different paths): **{}**.\n\n",
        h.duplicates.content_duplicate_tabs.len()
    ));
    for g in &h.duplicates.content_duplicate_tabs {
        s.push_str(&format!("- {}\n", g.join(" == ")));
    }
    if !h.duplicates.content_duplicate_tabs.is_empty() {
        s.push('\n');
    }
    s.push_str("Source format distribution (chunks):\n\n| Format | Chunks |\n| --- | --- |\n");
    for (k, v) in &h.format_distribution {
        s.push_str(&format!("| {k} | {v} |\n"));
    }
    s.push('\n');

    s.push_str(
        "## Reading\n\nThe corpus is fully **measured** (structure / gesture / \
         complexity / rights) but sits at the **weakest source-identity level**: \
         every manifest is schema v8, so `sha256` and `track_index` are absent \
         everywhere. The fail-closed holdout contracts (Phase-0 audit; ADR-0031) \
         therefore currently block on the whole corpus — the immediate \
         prerequisite is the **v9 `sha256` backfill**, then **v10 `song_id`** \
         curation. The multi-transcription works above are the concrete leakage \
         ADR-0031 addresses. The two-manifest split (a small curated set and a \
         large ingest set) is the real shape the migration must handle.\n",
    );
    s
}

fn list_section(s: &mut String, title: &str, items: &[String], cap: usize) {
    if items.is_empty() {
        return;
    }
    s.push_str(&format!(
        "<details><summary>{title} ({})</summary>\n\n",
        items.len()
    ));
    for it in items.iter().take(cap) {
        s.push_str(&format!("- `{it}`\n"));
    }
    if items.len() > cap {
        s.push_str(&format!(
            "- … and {} more (see `corpus-health.json`)\n",
            items.len() - cap
        ));
    }
    s.push_str("\n</details>\n\n");
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stem_drops_the_last_extension() {
        assert_eq!(stem("Song.gp5"), "Song");
        assert_eq!(stem("A - B (ver 2).gpx"), "A - B (ver 2)");
        assert_eq!(stem("noext"), "noext");
    }

    #[test]
    fn version_suffix_is_stripped_only_when_it_is_a_version() {
        assert_eq!(strip_version_suffix("A - B (ver 2 by X)"), "A - B");
        assert_eq!(strip_version_suffix("A - B (ver by X)"), "A - B");
        assert_eq!(strip_version_suffix("A - B (2)"), "A - B");
        // Not a version marker — preserved.
        assert_eq!(strip_version_suffix("Song (Reprise)"), "Song (Reprise)");
        assert_eq!(strip_version_suffix("Plain Title"), "Plain Title");
    }

    #[test]
    fn work_key_groups_transcriptions_of_one_composition() {
        let a = work_key("A Lot Like Birds - Connector.gp5");
        let b = work_key("A Lot Like Birds - Connector (ver 2 by LPFzCS_LMS).gp5");
        let c = work_key("A Lot Like Birds - Connector (ver 3 by someone).gpx");
        assert_eq!(a, "a lot like birds - connector");
        assert_eq!(a, b, "base and (ver 2 …) must share a work key");
        assert_eq!(b, c, "different transcriber versions must share a work key");
        // A different song must not collapse into the same work.
        assert_ne!(a, work_key("A Lot Like Birds - Divisi.gp5"));
    }

    #[test]
    fn resolve_counts_exact_then_stem() {
        let mut by_basename: BTreeMap<String, Vec<String>> = BTreeMap::new();
        by_basename.insert("x.gp5".to_owned(), vec!["a/x.gp5".to_owned()]);
        let mut by_stem: BTreeMap<String, Vec<String>> = BTreeMap::new();
        by_stem.insert(
            "y".to_owned(),
            vec!["a/y.gpx".to_owned(), "b/y.gp5".to_owned()],
        );
        assert_eq!(resolve("x.gp5", &by_basename, &by_stem), 1); // exact basename
        assert_eq!(resolve("y.gp5", &by_basename, &by_stem), 2); // ambiguous via stem
        assert_eq!(resolve("missing.gp5", &by_basename, &by_stem), 0);
    }

    #[test]
    fn walk_errors_on_a_missing_directory() {
        assert!(walk(Path::new("definitely/not/a/real/dir/xyzzy")).is_err());
    }
}
