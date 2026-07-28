# v9 corpus backfill — migration run report (PR C)

**Date:** 2026-07-28
**Tool:** `migrate/` (`migrate-v9`), merged in #162
**Scope:** execute the v9 `sha256` backfill on a **fresh copy** of `~/griff_data`
and verify it. This report is the evidence; it does **not** modify the live
corpus. Cutover of the production corpus is a separate, separately-authorized
step.

This is **PR C** of the corpus-modernization arc: A (census, #158) → B
(migration contract + tool, #162) → **C (run + verify)** → D (v10 `song_id`,
blocked on ADR-0031 acceptance).

## Inputs

A byte copy of `~/griff_data` (`corpus/` + `tabs/`), migrated into a fresh output
directory — never in place.

| Input | Count | Schema |
|---|---|---|
| standalone `*.chunk.json` | 9907 | v8 |
| `corpus/manifest.json` (curated) | 220 chunks | v8 |
| `corpus/ingest/manifest.json` (ingest) | 9687 chunks | v8 |
| source tabs under `tabs/` | 410 | — |

220 + 9687 = 9907 manifest chunk records, matching the 9907 standalone files.

## Command

```sh
migrate-v9  <copy>/corpus  <copy>/tabs  <out>
# → migrated 9907 chunk file(s) + 2 manifest(s) → <out>
```

Zero refusals: every chunk's `source.filename` resolved to exactly one tab. (The
20 source tabs that also sit at the `corpus/` root were not needed — every
referenced source lives under `tabs/`.)

## Results

### Coverage

| Metric | standalone (9907) | curated (220) | ingest (9687) |
|---|---|---|---|
| `sha256` present | **9907 (100%)** | 220 (100%) | 9687 (100%) |
| `track_index` present | **0 (0%)** | 0 | 0 |

`track_index` is **0% by design** — the migrator never guesses it (guessing `0`
would reload a second-guitar chunk as the first). Its recovery is separate
follow-up work.

### Correctness

Each recorded `sha256` was independently recomputed as the SHA-256 of the tab the
chunk resolves to (basename, then extension-insensitive stem):

- standalone: **9907 / 9907** correct
- curated manifest: **220 / 220** correct
- ingest manifest: **9687 / 9687** correct

### Structural diff (only sha256 + schema changed)

Per-record semantic diff over all 9909 records: stripping the newly-added
`source.sha256` key from each migrated record reproduces the pre-migration record
**exactly**.

- standalone: **9907 / 9907** structurally identical apart from the added key
- both manifests: identical after re-downgrading `schema_version` 9→8 and
  stripping the added keys — i.e. the **only** manifest change is
  `schema_version 8 → 9` plus the per-chunk `sha256`.

Record-only tree digests (9909 `*.chunk.json` + `manifest.json` files, sorted):

```
before: c004d5225b51563137d538495f076e6e4f1086f407a1de2c8372fbcd3d186194
after : 810421840463560b78233ebe79e89d05101b3ca14a0ee29a7774dc500cafd790
```

The migrator emits **records only** (9909 files). Non-record files — the 20
`corpus/`-root source tabs and the `_inventory/` artifacts — are not part of its
output; a production cutover carries them over verbatim.

## Census, before vs after

Re-ran the census (#158) on both the v8 copy and the v9 output, same `tabs/`.

| census block | before → after |
|---|---|
| `v9_coverage.sha256_present` | **0 → 9907** |
| `v9_coverage.track_index_present` | 0 → 0 |
| `input_digest` | `6955b33b…` → `e2b72750…` (changed — chunk bytes changed) |
| `manifests` | v8 → v9 (schema bump) |
| `counts` | **identical** (9907 chunk files, 9907 records, 400 referenced names, 410 tabs) |
| `annotation_coverage`, `duplicates`, `ensemble_integrity`, `format_distribution`, `manifest_reconciliation`, `source_reconciliation`, `v10_readiness` | **all identical** |

The migration touched exactly the source-identity fields and nothing else. The
v9 census artifact is committed alongside this report as
[`2026-07-v9-corpus-health.json`](2026-07-v9-corpus-health.json).

## File-level holdout smoke

`HoldoutTargetSourceFile` is CONDITIONAL PASS in the metric inventory: it needs
`source.sha256` and must **fail closed** on any sha256-less record. Running the
predicate over both corpora:

| | v8 (before) | v9 (after) |
|---|---|---|
| chunks with `source.sha256` | 0 / 9907 | **9907 / 9907** |
| predicate | **FAIL CLOSED** (9907 keyless) | **PASS** |
| distinct source files (by `sha256`) | — | 399 |
| deterministic ~20% holdout | — | 77 sources → 1792 chunks |
| train | — | 322 sources → 8115 chunks |
| source files split across both sides (leakage) | — | **0** |
| unassigned chunks | — | 0 |

The migration flips file-level holdout from fail-closed to a clean source-keyed
split with zero chunk leakage — the holdout law (split by source identity, never
by chunk) holds.

### Incidental finding

400 referenced filenames resolve to **399** distinct digests: two differently
-spelled Underoath filenames (`… Ive Got Ten Friends …` / `… I Got 10 Friends …`)
are **byte-identical**. The migrator's "duplicate bytes are not ambiguous" rule
assigned both the same digest without a conflict. This is also a latent
same-recording signal for the eventual `song_id` / holdout-by-song work.

## What this does *not* do

- **No production cutover.** This ran on a copy; replacing the live corpus is a
  separate authorized step.
- **No `track_index`.** Left absent everywhere; recovery is separate work.
- **No v10 `song_id`.** Blocked until ADR-0031 is Accepted.

## Reproduce

```sh
# on a copy of ~/griff_data (never in place):
cargo build --release --manifest-path migrate/Cargo.toml
migrate/target/release/migrate-v9  <copy>/corpus  <copy>/tabs  <out>

# census before/after:
cargo build --release --manifest-path census/Cargo.toml
census/target/release/census  <copy>/corpus  <copy>/tabs  before.json  before.md
census/target/release/census  <out>          <copy>/tabs  after.json   after.md
```
