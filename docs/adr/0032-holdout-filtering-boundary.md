# ADR 0032: Filter holdout over LoadedChunk before corpus material, owned by the offline Reachability Lab

Date: 2026-07-29
Status: Proposed

## Context

The generator loads a corpus into `Vec<LoadedChunk>` and immediately folds it
into `CorpusMaterial`. Each [`LoadedChunk`](../../core/src/generation_input.rs)
(`core/src/generation_input.rs:65`) carries the full `ChunkMeta` — `sha256`,
`song_id` (v10, ADR-0031), `bar_range`, and `track_index`. But
`corpus_material` (`core/src/generation_input.rs:137`) discards that provenance,
folding the chunks into anonymous rhythm templates, novelty references, gesture
stats, and skipped names. The CLI loader does exactly this with no decision in
between: it pushes each prepared chunk onto `loaded`
(`cli/src/generation_input.rs:64`) and calls `corpus_material(loaded, skipped)`
(`cli/src/generation_input.rs:68`). It scans standalone `*.chunk.json` records
and compiles the successful ones — it **never reads a `CorpusManifest`**.

There is therefore **no point at which a holdout decision can be made**: once
material is compiled, `song_id` / `sha256` / `bar_range` are gone. ADR-0031
(Accepted) added `song_id` and `song_holdout_preflight`
(`core/src/corpus.rs`), making song-level holdout *implementable* fail-closed —
but nothing calls it in a generation path, so the capability is inert. Note the
preflight takes a complete `CorpusManifest` (every manifest chunk, plus the
optional `songs` map), which the current CLI path does not even load.

The reachability proposal is non-binding discussion material; its Phase-0 audit
([`../audit/2026-07-generator-reachability-metric-inventory.md`](../audit/2026-07-generator-reachability-metric-inventory.md)
§3–4) recommends fixing the measurement/holdout boundary and isolated
offline-lab ownership in an ADR before implementation, and explicitly defers
roadmap-stage placement — an isolated, lab-style instrument comes first. The
audit's doctrine (§3; proposal §6): provenance exclusion is **primary** and
happens **before** material construction, and holdout failures **abort** (typed
refusal), never degrade into conveniently-shrunk "diagnostics".

## Decision

We fix the holdout **execution boundary**, its authoritative input, and its
ownership. We bind no measurement axes and change no generation behaviour.

1. **Ownership and placement.** `CorpusMode`, `TargetIdentity`, `HoldoutError`,
   and the boundary orchestration live in the **offline Reachability Lab** — an
   isolated, lab-style instrument (the `fuzz` / `lab` isolation precedent,
   ADR-0010). Production CLI and cockpit paths acquire **no** holdout policy;
   core production generation and reranking are unchanged; ordinary generation
   bypasses this seam entirely. Any loader addition exists solely to hand the lab
   one authoritative corpus, not to duplicate policy across frontends. **No
   roadmap stage number is assigned** — the audit defers placement.

2. **Authoritative input — one source of truth.** The boundary consumes a single
   bound corpus, conceptually:

   ```rust
   struct LoadedCorpus {
       manifest: CorpusManifest,   // authoritative song_id facts + optional songs map
       loaded: Vec<LoadedChunk>,   // records prepared for material
       skipped: Vec<String>,       // names that could not be loaded
   }
   ```

   The manifest and the loaded records **must describe the same corpus**: each
   `LoadedChunk.meta` originates from — or exactly matches, by `ChunkId` — one
   `manifest.chunks` entry, and a duplicate, missing, or provenance-mismatched
   record is a **typed refusal**. (Equivalently, the lab may build `loaded`
   directly from `manifest.chunks`, which satisfies the invariant by
   construction.) This closes the stale-manifest leak: preflight and filtering
   must read the **same** `song_id` facts, so a manifest asserting
   `shaA → song1` can never validate a run that a separately-read record would
   filter as `shaA → song2`.

3. **Boundary — absence-capable result.** The smallest pure seam:

   ```rust
   fn prepare_corpus_for_mode(
       corpus: LoadedCorpus,
       mode: CorpusMode,
       target: &TargetIdentity,
   ) -> Result<Option<CorpusMaterial>, HoldoutError>
   ```

   `Ok(None)` is a genuinely corpus-free run — the generation API already models
   this as `material: None`, disabling corpus rhythms, references, and gesture.
   `Ok(Some(_))` is a corpus-backed run. The modes are **distinct experiments**:

   - **`NoCorpus`** → `Ok(None)` — no corpus at all.
   - **`LeakyDiagnostic`** → `Ok(Some(corpus_material(all_loaded, skipped)))` — a
     corpus is deliberately supplied **unfiltered**, named so it can never be
     mistaken for a holdout.
   - **`HoldoutTargetSong`** → preflight → filter →
     `Ok(Some(corpus_material(filtered, skipped)))`.

   `NoCorpus` (no corpus) and `LeakyDiagnostic` (an unfiltered corpus) are
   **not** the same experiment, and neither is a "no-holdout" alias.

4. **`HoldoutTargetSong` contract.** In order:
   1. run `song_holdout_preflight` over the **entire participating corpus**
      (`corpus.manifest`, including its optional `songs` map);
   2. typed-refuse on any coverage or identity inconsistency — the existing
      `SongHoldoutRefusal` set (unidentified / uncurated / inconsistent-`sha256`
      / manifest disagreement) propagates unchanged;
   3. require a target `song_id`;
   4. exclude **every** `LoadedChunk` whose `SourceRef.song_id` equals the target
      (all representations of that work — MIDI, GP editions, covers);
   5. only then compile the remaining chunks into `CorpusMaterial`.

5. **Target identity carries explicit per-mode provenance**, not a vague source
   string: `source_sha256`, `song_id`, `bar_range`, `track_index`, `projection`,
   and `eligibility` (the audit §4 fields), so the file, fragment, and song modes
   each select on the correct identity level.

6. **Non-goals (bound).** No production generation-behaviour change; no
   scoring / rerank change; no curation or automatic song grouping; **no
   post-hoc relabelling of a failed or absent holdout as a valid one**; filtering
   is deterministic and does not mutate metadata.

7. **First implementation slice.** Song mode plus the minimal `CorpusMode` /
   `TargetIdentity` / `HoldoutError` / `LoadedCorpus` scaffolding. File and
   fragment modes follow as **separate slices** — their overlap semantics,
   notably `bar_range == None` meaning whole-source overlap, deserve their own
   tests rather than riding along as bonus complexity.

The implementation is a separate red→green slice once this ADR is accepted:
synthetic-fixture characterization first, then the boundary function. This ADR
binds nothing until accepted.

## Consequences

**Good / possible (after the implementation slice lands).**

- Song-level holdout becomes executable end-to-end: an uncurated corpus will
  **typed-refuse before material construction**, and a fully curated (synthetic)
  corpus will exclude every representation of the target work **before any
  material channel** (rhythm templates, novelty references, gesture stats) is
  compiled — with the single-authority invariant preventing a stale-manifest
  leak.
- The boundary will be a single pure function over `LoadedCorpus`, testable with
  synthetic fixtures and independent of any curation data — the executable
  consumer and refusal path will land before labeling begins.
- The provenance-primary, fail-closed doctrine will be enforced in code, not
  merely documented, and `NoCorpus` / `LeakyDiagnostic` / `HoldoutTargetSong`
  will be distinguishable experiments rather than one overloaded "no-holdout"
  path.

**Bad / cost.**

- One more seam between load and material, an authoritative `LoadedCorpus` the
  lab must assemble (manifest bound to loaded records), and a lab-owned mode enum
  and error type.
- Until curation exists, a song holdout will **refuse on the real corpus**. This
  is correct (fail-closed over an uncurated population) but means the executable
  path deliberately precedes any real run — the consumer and refusal exist before
  the data.

**Impossible / explicitly out of scope.**

- No file or fragment mode here (separate slices); no eligibility / projection
  axis *implementation* (the boundary only references them); no curation,
  production cutover, or `track_index` recovery; no generation or scoring change;
  no holdout policy in the production CLI or cockpit.

Sequence this enables (each a later, separately-contracted step): the holdout
wiring slice → a `song_id` curation tool and policy (suggestion-only, human
confirmation, manifest generation, consistency validation) → incremental,
measured corpus curation → the first real song-holdout run on a curated subset →
only later, production cutover or `track_index`, each under its own contract.
