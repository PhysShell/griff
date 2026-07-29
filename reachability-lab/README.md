# griff-reachability-lab — holdout-filtering boundary (ADR-0032)

An **isolated** offline instrument (ADR-0010 / ADR-0032): deliberately **not** a
workspace member, so production builds, CI, `--workspace` clippy, and the
CLI/cockpit never acquire holdout policy. It owns the boundary between corpus
loading and material construction.

## Why here

The generator loads `Vec<LoadedChunk>` — each carrying full `ChunkMeta`
(`sha256`, `song_id`, `bar_range`, `track_index`) — and immediately folds it into
`CorpusMaterial`, discarding that provenance into anonymous rhythm templates,
novelty references, and gesture stats. So a holdout decision can only be made
**over `Vec<LoadedChunk>`, before `corpus_material`**. This crate is that seam.

## Scope (ADR-0032 first slice)

**Song and source-file modes**, fail-closed, over a single-authority
`LoadedCorpus`:

```rust
prepare_corpus_for_mode(corpus: LoadedCorpus, mode: CorpusMode, target: &TargetIdentity)
    -> Result<Option<CorpusMaterial>, HoldoutError>
```

- **`NoCorpus`** → `Ok(None)` — a genuinely corpus-free run.
- **`LeakyDiagnostic`** → `Ok(Some(_))` — a corpus supplied deliberately
  **unfiltered**, named so it can never be mistaken for a holdout.
- **`HoldoutTargetSong`** → bind → song preflight → require target `song_id` →
  exclude every representation of that song → compile the survivors.
- **`HoldoutTargetSourceFile`** → bind → source-file preflight → require target
  `source_sha256` → exclude every chunk cut from that file (by exact `sha256`,
  regardless of `bar_range` or track) → compile the survivors. Source-file mode
  needs **only** complete hash identity — it does **not** run the song preflight
  and does **not** require `song_id`, so a hash-identified but song-uncurated
  corpus is a valid file-mode experiment. Its preflight rejects any participating
  chunk without `sha256` (no basename fallback).

`LoadedCorpus { manifest, loaded, skipped }` is **one authority**: every loaded
record maps by `ChunkId` to exactly one manifest chunk whose **full** `ChunkMeta`
matches (not a hand-picked two-field imitation), no id is duplicated in the
manifest or the loaded set — otherwise the preflight would validate one dataset
while the filter executes another (the stale-manifest leak).

A holdout that would exclude nothing is a **refusal**, not a silent success: if
the target is carried by no *loaded* chunk — including when its only source
failed to load and sits in `skipped` — the run typed-refuses (`TargetSongAbsent`
/ `TargetSourceAbsent`) rather than returning the corpus unchanged. Every binding
violation, missing/absent target, and preflight refusal is **typed**
(`HoldoutError` / `BindingRefusal` / `SourceHoldoutRefusal`), never a silent pick.

The zero-leakage guarantee is proven by asserting the held-out material equals
what the keeper alone produces across **all** channels — references, rhythm
templates, and gesture.

## Not in this crate

- Fragment mode (its range-overlap semantics — including the dangerous rule that
  `bar_range == None` overlaps every range in the same source — get their own
  slice, built over this source-file identity path).
- Any measurement / eligibility / projection axis.
- Curation, production wiring, generation or scoring changes.

## Run

```sh
cargo test --manifest-path reachability-lab/Cargo.toml
```
