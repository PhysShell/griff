# ADR 0031: Add a curator-assigned canonical song identity to the corpus

Date: 2026-07-26
Status: Accepted

## Context

The corpus model (schema v9, `core/src/corpus.rs`) identifies a chunk's origin
at three levels, none of which is the **composition**:

- `ChunkId` — the chunk;
- `SourceRef.sha256` (v9) — the **source file** (a content hash; "a filename is
  not an identity", per the v9 note), with `filename`, `track_index`, and an
  optional `bar_range`;
- `EnsembleRef.group_id` (v4) — sibling parts of **one source span** (e.g. the
  two guitars of a DGD section), explicitly *not* a whole song.

There is no field that says "these source files are the same song". The
`ChunkMeta.title` is a free-form string, not a stable identifier. So two
transcriptions of one composition — a MIDI and a Guitar Pro file, or two GP
editions — have different `sha256` values and cannot be linked.

The 2026-07 Generator Reachability Lab Phase 0 audit
([`../audit/2026-07-generator-reachability-metric-inventory.md`](../audit/2026-07-generator-reachability-metric-inventory.md)
§3) proved the concrete consequence: **song-level holdout is not
implementable fail-closed**. `HoldoutTargetSourceFile` and
`HoldoutTargetFragment` work on `sha256` + `bar_range`, but `HoldoutTargetSong`
("exclude every chunk of the target song") would silently keep one
representation while excluding another — a source-*file* holdout mislabelled as
a *song* holdout. The same gap undermines source-identity splits for the human
similarity benchmark
([`../proposals/preference-and-similarity-learning.md`](../proposals/preference-and-similarity-learning.md),
[`../proposals/human-similarity-benchmark.md`](../proposals/human-similarity-benchmark.md)),
whose "split by source identity, never by chunk" law is only as strong as the
identity it splits on. `EnsembleRef.group_id` cannot stand in: its contract is
one source span, so two files of one song get different groups.

Prior art (recorded in `decisions.log.md`, prior-art-first rule): the
library/MIR world already models this as distinct levels. **FRBR**
(Work → Expression → Manifestation → Item) and **MusicBrainz** (Work ↔
Recording ↔ Release/Track, with **ISWC** as a work code) both separate the
abstract *work* from its concrete *manifestations*. Griff already has the lower
levels: `sha256` is a Manifestation-level identity (a specific file), and a
chunk is Item-ish. What is missing is the **Work** level — the composition that
several files transcribe.

## Decision

We add an **optional, curator-assigned canonical song identity** to the corpus,
following the established optional-field, forward-compatible schema pattern
(schema **v10**), and we bind holdout semantics to it.

- **New field.** `SourceRef` gains `song_id: Option<SongId>` (schema v10), where
  `SongId(String)` is a stable, opaque, curator-assigned identifier of a
  **composition** (a Work). It lives on `SourceRef` because a *source file* is a
  transcription of one song; `sha256` already identifies the file, and `song_id`
  names the work above it. Pre-v10 records lack the key, load it as `None`, and
  re-serialize byte-identically — exactly as `rights` (v7), `EnsembleRef` (v4),
  and `sha256`/`track_index` (v9) did.

- **Curator-assigned, never derived.** Like rights (v7), `song_id` is a fact
  about provenance that content cannot yield: two files are "the same song"
  because the curator asserts it, not because a metric found them similar. It is
  captured at curation time and is not backfilled by any analysis. A tool may
  *suggest* groupings (e.g. from `title` + artist), but the stored value is a
  human decision.

- **Exact scope — what `song_id` claims and does not.** `song_id` groups source
  files that are the **same composition** for the single purpose of
  **leakage-safe holdout and source-identity splits**. It makes **no** claim of
  cover-detection, arrangement equivalence, or musical similarity, and it is
  **not** a production-scoring signal. Same `song_id` means "do not let these
  leak across a train/eval boundary", nothing more.

- **`song_id` is Work identity, with no musical-distinctness override.** Every
  manifestation, arrangement, edition, and cover of one composition **must** carry
  the same `song_id`; musical or arrangement distinctness does **not** change Work
  identity. Allowing a curator to split a cover onto a different `song_id`
  because it "sounds different" would turn the field back into a similarity /
  arrangement grouping and let one work re-cross the holdout boundary — the exact
  leak this ADR closes. If separating expressions or arrangements ever becomes
  useful, that is a *separate* `arrangement_id` / `expression_id` (a lower FRBR
  level) or an explicit split policy; one field does not play both FRBR Work and
  a taste judgement.

- **Fail-closed holdout, formalized.** The Phase-0 audit's three modes become a
  bound contract:
  - `HoldoutTargetSourceFile` / `HoldoutTargetFragment` — require `sha256`;
    `sha256`-less records fail closed (reject or migrate), never trusted by
    basename.
  - `HoldoutTargetSong` — requires `song_id` **coverage over the corpus**, not
    merely a labelled target. Excluding only sources whose `song_id` equals the
    target's is *not* fail-closed: a pre-v10 or uncurated record with
    `song_id = None` may be an alternate transcription of the *same* composition,
    and an equality filter would silently retain it — re-leaking the held-out
    song. The **initial contract is strict refusal**: the preflight **MUST
    refuse the run** when any participating unique source lacks `song_id`, before
    material construction. (Conservatively *excluding* all `None` sources instead
    is also leakage-safe, but it silently shrinks the corpus and changes the
    benchmark population, so it is a **separate, explicitly versioned policy**,
    not the default.) A song holdout then excludes every chunk whose source
    carries the target `song_id`, with no unidentified source surviving to leak.

- **Corpus identity invariants (checked in the same preflight).** Because
  `SourceRef` is stored per chunk while `sha256` identifies the *file*, the
  preflight validates, before any holdout:
  1. all chunks sharing a `sha256` carry the **same** `song_id` (else one file
     gets split `song_id`s across chunk records and a song holdout excludes only
     part of the file — chunk-level leakage re-introduced through the very field
     meant to stop it);
  2. no `sha256` maps to two different `SongId`s;
  3. if the `songs` manifest is present, `manifest ↔ SourceRef.song_id` agree
     **both ways** (every manifest entry has matching sources, every labelled
     source is enumerated).
  A violation is a typed refusal, not a silent pick.

- **Manifest cross-check (defence-in-depth, optional).** The `songs` map
  (`SongId → [sha256]`) is a *convenience and typo check*, not the coverage
  proof: corpus-wide coverage is proven by the preflight scan of every
  participating source's `song_id` (invariant 1–2), which runs whether or not a
  manifest exists. The per-source field is authoritative.

The implementation is a separate red→green slice once this ADR is accepted:
characterization tests first (SPEC hard rule 5 — schema round-trip must not
change for pre-v10 files), then the v10 field + the holdout-mode contract. This
ADR binds nothing until accepted (`docs/adr/README.md`).

## Consequences

**Good / possible.**

- Song-level holdout becomes implementable fail-closed, unblocking the Phase-0
  audit's `HoldoutTargetSong` and the benchmark's source-identity splits — the
  Phase-1 prerequisite the audit named.
- Dedup and provenance gain a Work level: "another transcription of this song"
  is answerable, where today only "the same file" (`sha256`) is.
- The identity ontology is now explicit and layered (Work = `song_id`,
  Manifestation = `sha256`, span = `EnsembleRef`, chunk = `ChunkId`), so future
  features reason about the right level instead of overloading `title`.

**Bad / cost.**

- `song_id` is curation labour and can be wrong (a mislabelled or missing group
  leaks or over-excludes). It is a human assertion, with the usual curation-data
  fallibility; the manifest cross-check mitigates typos, not judgement.
- One more optional schema field to carry through the loader, serializer, and
  curation tooling (bounded — the pattern is well-worn, v4/v7/v9).

**Impossible / explicitly out of scope.**

- `song_id` does not make griff a cover-detection or musical-work-matching
  system; it records curator groupings, not discovered ones.
- It grants no production-scoring authority and changes no generation behaviour;
  it is provenance for holdout and dedup only.
- It does not retro-link existing corpus files: pre-v10 records remain
  `song_id = None`. Under the **default** `HoldoutTargetSong` policy the preflight
  **refuses the entire run** until every participating unique source is curated;
  conservative *exclusion* of unknown sources is permitted only under a separate,
  explicitly versioned policy (Decision). Fail-closed, never silently backfilled.
