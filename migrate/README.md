# griff-corpus-migrate — v8 → v9 corpus migration (SHA-256 backfill)

An **isolated** offline tool (ADR-0010 precedent, like [`../lab`](../lab) and the
corpus census): deliberately **not** a workspace member, so production builds,
CI, `--workspace` clippy, and the deny.toml posture never touch it. It rewrites a
**copy** of the corpus into a fresh output directory — never in place.

## Why

The real corpus manifests are schema **v8**: their `SourceRef` records carry no
`sha256` and no `track_index`. The fail-closed holdout law
([the reachability-metric inventory](../docs/audit/2026-07-generator-reachability-metric-inventory.md))
needs the source digest to split by source identity, so a v8 corpus blocks
source-file / fragment holdout entirely. Backfilling `sha256` is the prerequisite
that unblocks it.

## Scope — `sha256` only

This tool backfills **`SourceRef.sha256`** and nothing else. For each chunk it
resolves `source.filename` to a unique tab on disk, hashes the bytes with
`griff_core::corpus::source_sha256`, and records the digest. Manifests also have
`schema_version` advanced to the current v9.

It **does not** set `track_index`. Recovering the exact source track of a pre-v9
chunk is a *classification* problem (single note-bearing track → infer; multiple
tracks → replay the ingest slicing and match; no unambiguous match → refuse), not
a mechanical backfill. Guessing `0` would silently reload a second-guitar chunk as
the first. So the field is **left `None`**, which preserves the loader's
documented pre-v9 fallback, and the `track_index` migration is tracked as separate
follow-up work.

## Contract (enforced by the tests in `src/main.rs`)

The pure planning helpers (`resolve_source`, `build_plan`, `apply_plan`) and the
whole-tool `run()` are both covered — the end-to-end tests build temporary
`corpus/` + `tabs/` trees and drive `run()` against them.

- **Unambiguous join** — a source resolves to exactly one tab (by basename, then
  by extension-insensitive stem). Byte-identical duplicate tabs are **not**
  ambiguous; their digest is determined.
- **Fail closed, no partial write** — if *any* chunk is unresolved (missing or
  ambiguous) or conflicts with a digest already recorded, the whole run refuses
  and writes nothing. An end-to-end test asserts a refused run creates no output.
- **Never in place** — before any input is read, a preflight canonicalizes all
  three roots (resolving symlink aliases) and refuses when the output already
  exists, equals an input root, or nests with either input root in either
  direction. Every such refusal leaves the inputs byte-identical.
- **Pinned target** — the migrator writes exactly schema **v9**
  (`TARGET_SCHEMA_VERSION`), never the floating `griff_core` `SCHEMA_VERSION`, so
  a recompile after v10 cannot silently forge a v10 stamp. Inputs newer than the
  target are refused, not downgraded.
- **Idempotent** — re-running over already-backfilled records is a no-op.
- **Consistent** — two chunks naming the same source receive the same digest.
- **Verifiable** — the recorded digest equals what the loader recomputes
  (`source_sha256`), asserted against `sha256sum`.

## Usage

```sh
cargo run --manifest-path migrate/Cargo.toml -- <corpus_dir> <tabs_dir> <out_dir>
```

`<corpus_dir>` is walked for `*.chunk.json` records and `manifest.json` files;
`<tabs_dir>` is walked for the source tabs; migrated copies are written under
`<out_dir>` with the input's relative layout preserved.

## Running it for real (follow-up)

This crate is the migration **contract + tool**. Executing it against the real
corpus is deliberately a separate step: it runs on a **backup copy** of
`~/griff_data`, never on the working tree, and its output is reviewed before it
replaces anything.
