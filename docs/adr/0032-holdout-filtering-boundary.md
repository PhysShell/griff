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
(`cli/src/generation_input.rs:68`).

There is therefore **no point at which a holdout decision can be made**: once
material is compiled, `song_id` / `sha256` / `bar_range` are gone. ADR-0031
(Accepted) added `song_id` and `song_holdout_preflight`
(`core/src/corpus.rs`), making song-level holdout *implementable* fail-closed —
but nothing calls it in a generation path, so the capability is inert.

The reachability proposal is non-binding discussion material; its Phase-0 audit
([`../audit/2026-07-generator-reachability-metric-inventory.md`](../audit/2026-07-generator-reachability-metric-inventory.md)
§3–4) recommends fixing the measurement/holdout boundary and isolated
offline-lab ownership in an ADR before implementation, and explicitly defers
roadmap-stage placement — an isolated, lab-style instrument comes first. The
audit's doctrine (§3; proposal §6): provenance exclusion is **primary** and
happens **before** material construction, and holdout failures **abort** (typed
refusal), never degrade into conveniently-shrunk "diagnostics".

## Decision

We fix the holdout **execution boundary** and its ownership. We bind no
measurement axes and change no generation behaviour.

1. **Ownership.** The offline Reachability Lab owns `CorpusMode` and all holdout
   run artifacts. It is an isolated, lab-style instrument (the `fuzz`/`lab`
   isolation precedent, ADR-0010), not a production generation feature. **No
   roadmap stage number is assigned here** — the audit defers placement.

2. **Boundary.** Holdout filtering occurs over `Vec<LoadedChunk>`, **before**
   `corpus_material` and before any rhythm / reference / gesture compilation —
   the only point where full provenance is still present. The smallest pure
   seam:

   ```rust
   fn prepare_corpus_for_mode(
       loaded: Vec<LoadedChunk>,
       skipped: Vec<String>,
       mode: CorpusMode,
       target: &TargetIdentity,
   ) -> Result<CorpusMaterial, HoldoutError>
   ```

   A `NoCorpus` / no-holdout mode passes straight through to `corpus_material`
   unchanged, so ordinary generation is byte-for-byte as today.

3. **`HoldoutTargetSong` contract.** For the song mode, in order:
   1. run `song_holdout_preflight` over the **entire participating corpus**;
   2. typed-refuse on any coverage or identity inconsistency — the existing
      `SongHoldoutRefusal` set (unidentified / uncurated / inconsistent-`sha256`
      / manifest disagreement) propagates unchanged;
   3. require a target `song_id`;
   4. exclude **every** `LoadedChunk` whose `SourceRef.song_id` equals the target
      (all representations of that work — MIDI, GP editions, covers);
   5. only then compile the remaining chunks into `CorpusMaterial`.

4. **Target identity carries explicit per-mode provenance**, not a vague source
   string: `source_sha256`, `song_id`, `bar_range`, `track_index`, `projection`,
   and `eligibility` (the audit §4 fields), so the file, fragment, and song
   modes each select on the correct identity level.

5. **Non-goals (bound).** No production generation-behaviour change; no
   scoring / rerank change; no curation or automatic song grouping; **no
   post-hoc relabelling of a failed or absent holdout as a valid one**; filtering
   is deterministic and does not mutate metadata.

6. **First implementation slice.** Song mode plus the minimal `CorpusMode` /
   `TargetIdentity` / `HoldoutError` scaffolding. File and fragment modes follow
   as **separate slices** — their overlap semantics, notably `bar_range == None`
   meaning whole-source overlap, deserve their own tests rather than riding along
   as bonus complexity.

The implementation is a separate red→green slice once this ADR is accepted:
synthetic-fixture characterization first (the current uncurated corpus refuses
before material construction; a curated fixture excludes every representation of
the target song with zero leakage into rhythms, references, and gesture; a
missing target `song_id` and any unidentified / uncurated / inconsistent source
typed-refuse; unrelated songs remain; filtering is deterministic and leaves
metadata untouched; `NoCorpus` and any explicitly-named leaky diagnostic stay
distinct), then the boundary function. This ADR binds nothing until accepted.

## Consequences

**Good / possible.**

- Song-level holdout becomes executable end-to-end: the current uncurated corpus
  correctly **refuses** before material construction, and a curated corpus
  excludes every representation of the target work with **zero leakage** into any
  material channel (rhythm templates, novelty references, gesture stats).
- The boundary is a single pure function, testable with synthetic fixtures and
  independent of any curation data — the executable consumer and refusal path
  land before labeling begins.
- The provenance-primary, fail-closed doctrine is enforced in code, not merely
  documented.

**Bad / cost.**

- One more seam between load and material, and the lab owns a mode enum and an
  error type.
- Until curation exists, a song holdout **refuses on the real corpus**. This is
  correct (fail-closed over an uncurated population) but means the executable
  path deliberately precedes any real run — the consumer and refusal exist before
  the data.

**Impossible / explicitly out of scope.**

- No file or fragment mode here (separate slices); no eligibility / projection
  axis *implementation* (the boundary only references them); no curation,
  production cutover, or `track_index` recovery; no generation or scoring change.

Sequence this enables (each a later, separately-contracted step): the holdout
wiring slice → a `song_id` curation tool and policy (suggestion-only, human
confirmation, manifest generation, consistency validation) → incremental,
measured corpus curation → the first real song-holdout run on a curated subset →
only later, production cutover or `track_index`, each under its own contract.
