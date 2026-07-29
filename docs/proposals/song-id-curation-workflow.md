# Song-ID Curation Workflow (proposal)

Status: for discussion (a proposal — it binds nothing and is **not** Accepted).

An offline, **human-confirmed** workflow for assigning `SongId` (ADR-0031,
schema v10) to corpus sources, so that song-level holdout
([ADR-0032](../adr/0032-holdout-filtering-boundary.md);
[`../../reachability-lab`](../../reachability-lab)) becomes usable without
letting heuristics become provenance facts by clerical accident. This document
designs the workflow; it does not build it, and it writes no corpus labels.

## 1. Fixed inputs — ADR-0031 and ADR-0032 are law here

This proposal treats the following as **fixed**, not material to reinterpret:

1. `SongId` identifies a composition at the **Work** level.
2. It is **opaque** and **curator-assigned**.
3. It is **never** derived automatically from musical or metadata similarity.
4. A tool may generate **suggestions**, but suggestions have **no authority**.
5. Every edition, transcription, arrangement, and cover of one composition uses
   the **same** `SongId`.
6. `song_id` exists **only** for leakage-safe holdout and source-identity splits.
7. Per-`SourceRef.song_id` values are **authoritative** after application.
8. `CorpusManifest.songs` is a deterministic **convenience and cross-check**, not
   the source of truth.
9. A holdout-ready corpus requires **complete, consistent** song coverage.
10. Unknown or inconsistent identity must **typed-refuse**, never be guessed.

No arrangement-level exception is invented. Separating expressions/arrangements
is a *separate* future `arrangement_id` (a lower FRBR level), out of scope here
(ADR-0031).

## 2. Goal

A pipeline that can:

```
inventory sources
  → generate non-authoritative grouping suggestions
  → record explicit curator decisions
  → build a deterministic application plan
  → apply that plan transactionally to a fresh corpus copy
  → generate the songs manifest
  → validate consistency and holdout readiness
```

The system reduces curation **labour**; it never lets a heuristic promote itself
to a stored identity.

## 3. Curation unit — the source file, by `sha256`

Curation operates on **unique source files identified by exact `sha256`**, never
on individual chunks. All chunks sharing one `sha256` are **one indivisible
source unit**:

- they must receive the **same** `SongId`;
- a decision may **not** label only some chunks of a source;
- conflicting existing labels on one source **typed-refuse**
  (`ConflictingExistingSongIds`);
- a source without `sha256` is **not curatable** by this tool and typed-refuses
  (`UnidentifiedSource`).

`ChunkMeta.title`, filename, format, `bar_range`, `track_index`, and chunk ids
are **display / suggestion evidence** only; they do not define source identity.

## 4. Ownership — selected: a standalone offline `song-curation/` tool

**Selected:** a **standalone, isolated offline tool** (provisionally
`song-curation/`), following the established isolation posture of `fuzz` / `lab`
/ `census` / `migrate` / `reachability-lab` (ADR-0010):

- not a production-generation dependency;
- **no** holdout or curation policy added to the cockpit;
- **no** in-place corpus mutation;
- **no** automatic execution in ordinary generation;
- `griff-core` supplies only reusable **schema and validation contracts**
  (`ChunkMeta`, `SourceRef`, `CorpusManifest`, `SongId`, `source_sha256`, and the
  existing `song_holdout_preflight`).

### Comparison (required)

| Owner | For | Against | Verdict |
|---|---|---|---|
| **Standalone `song-curation/`** (selected) | Matches the isolated-tool precedent; a multi-phase ledger/plan/apply workflow is too large for a subcommand; no production surface acquires curation policy; can carry its own artifact schemas. | One more isolated crate; not exercised by workspace CI (per the isolation policy, verified locally). | **Selected.** |
| Extend `griff curate` | Reuses an existing curation entry point. | `griff curate` is interactive per-chunk cockpit-adjacent curation; song identity is a *source-level, ledgered, transactional* operation with a fingerprint/plan/apply contract that does not fit an interactive per-chunk command, and it would pull curation policy into a production binary. | Rejected. |
| Extend `griff manifest` | Manifest generation already emits `CorpusManifest`. | `griff manifest` folds chunks into a manifest; it is not a curation ledger and must stay a pure projection. Owning issuance/decisions there overloads it and adds policy to production. | Rejected for *ownership* (but see §9 for its eventual manifest role). |

Ownership is **selected now**, not deferred to implementation.

## 5. Workflow phases

### 5.1 Inventory (read-only)

Read a corpus snapshot and **collapse chunks by exact source `sha256`** into
deterministic source records with at least:

```
source_sha256
filenames          # sorted, unique
titles             # sorted, unique
formats            # sorted, unique
chunk_ids          # sorted
existing_song_ids  # sorted, unique (from SourceRef.song_id)
existing_manifest_membership  # SongIds naming this sha256 in CorpusManifest.songs
```

A source with **more than one** distinct existing `song_id` across its chunks →
`ConflictingExistingSongIds` (read-only refuses to summarize it as clean). A
chunk without `sha256` → `UnidentifiedSource`. Inventory writes nothing.

### 5.2 Suggest (non-authoritative)

Deterministic, **evidence-bearing** candidate groupings. **Version 1 uses
metadata evidence only:**

- normalized `ChunkMeta.title`;
- normalized source filenames / stems;
- repeated source names with format/version suffixes removed by a **documented**
  rule (reuse the census's `strip_version_suffix` convention: trailing `(...)`
  removed only when the inner text starts with `ver`, contains ` by `, or is
  all-digits — never for e.g. `(Reprise)`);
- already-confirmed identity relationships, when supplied.

**No canonical artist field exists in the schema.** "Artist" parsed from a title
or filename is a **suggestion heuristic**, reported as such in `evidence`; it is
not structured provenance. If an artist signal is ever wanted as structured
input, it must arrive as a **separate explicit input artifact**, not inferred.

**No** note-content similarity, embeddings, audio fingerprinting, cover
detection, or MIR classifier enters version 1. Every suggested group exposes its
**evidence** and **uncertainty**, refers to exact source hashes, and **never
writes `song_id`**.

### 5.3 Confirm (explicit)

A curator explicitly **accepts / rejects / splits / merges / manually defines**
source groupings. **No default action means acceptance.** A batch or unattended
run must **not** convert suggestions into decisions. Confirmation is captured in
the **decisions artifact** (§8) such that a later apply run can prove **every
stored label came from an explicit curator decision** (each decision names the
`source_sha256`s, the curator, and a timestamp; the apply report echoes the
decisions digest).

### 5.4 Plan (immutable)

Build an **immutable application plan** from:

- an exact **corpus fingerprint** (§10);
- a **versioned decisions artifact**;
- the **tool-policy version**.

The plan enumerates **every** intended source-level assignment and **every**
affected chunk. If the corpus fingerprint has changed since inventory/confirm,
planning or application **typed-refuses**
(`DecisionCorpusFingerprintMismatch` / `PlanCorpusFingerprintMismatch`) rather
than rebasing decisions onto a different snapshot.

### 5.5 Apply (transactional)

Apply **only** a previously validated plan. Required semantics:

- write to a **fresh output directory** (`OutputAlreadyExists` if it exists;
  `OutputWouldModifyInput` if it resolves inside/equal to an input — reuse the
  `migrate-v9` preflight discipline);
- **all-or-nothing**; no partial output presented as successful;
- update **every** chunk sharing an assigned `sha256`;
- preserve every unrelated field byte-for-byte where serialization permits
  (only `SourceRef.song_id` is added/changed);
- **deterministic** file order and JSON rendering;
- **idempotent** for already-correct labels;
- `ConflictingExistingSongIds` / `SourceAssignedToMultipleSongs` /
  `UnknownDecisionSource` typed-refuse;
- **never clear** an existing label unless an explicit curator **correction**
  authorizes it (`ExistingLabelReplacementNotAuthorized` otherwise).

A **correction is a new curator decision**, not heuristic reconciliation.

### 5.6 Generate manifest (deterministic)

Generate `CorpusManifest.songs` deterministically from the applied per-source
labels: `SongId → sorted, unique [sha256]`. The per-source `SourceRef.song_id`
remains **authoritative**; the map is the cross-check (law 8).

**Manifest ownership — selected:** for the **first implementation**, the
`song-curation/` tool **exclusively** owns song-aware manifest generation
(strategy 1), kept isolated until the workflow is proven. The **recommended
durable end state** is **strategy 2**: extend `griff manifest` to rebuild `songs`
from the authoritative per-source labels, as a **later, separately-accepted**
change.

> **Interim hazard (open issue for review).** `griff manifest` today always emits
> `songs: None` (`ui-core/src/corpus.rs`). Until strategy 2 lands, running plain
> `griff manifest` over a curated corpus would **silently erase** the curated
> `songs` cross-check. The first implementation must therefore treat the curated
> corpus as tool-owned output and **document that ordinary `griff manifest` must
> not be run against it** until strategy 2 exists. Strategy 3 (curated manifest at
> a distinct path + ordinary generation *refuses to overwrite* it) is the
> alternative interim guard if reviewers prefer a hard stop over documentation.

### 5.7 Validate

Validation **uses the existing core `song_holdout_preflight`**, not a near-copy.
It also reports:

```
unique_source_count
labelled_source_count
unlabelled_source_count
song_count
conflicting_source_count
manifest_disagreement_count
holdout_ready: true | false
```

`holdout_ready: true` is permitted **only** when the complete corpus passes the
existing strict preflight (`song_holdout_preflight(&manifest) == Ok(())`).

## 6. Partial curation

Incremental curation is **supported** — requiring 100% completion before the
first confirmed decision would guarantee nobody ever curates. But:

- partial output is explicitly marked **`holdout_ready: false`**;
- it is **not** described as a valid song-holdout corpus;
- uncurated sources remain `song_id: None`;
- a partial run **never** invents an "unknown" shared `SongId`;
- the complete gate remains `song_holdout_preflight == Ok(())`.

Two states are **distinct** and must never be conflated:

- a **valid partial curation snapshot** (`holdout_ready: false`, some sources
  still `None`);
- a **complete holdout-ready corpus** (`holdout_ready: true`, strict preflight
  passes).

## 7. `SongId` issuance

**Selected policy:** an **opaque, ledger-issued identifier** —
`song-` + a zero-padded monotonic counter maintained in the decisions ledger
(e.g. `song-000042`) — issued **once** at human confirmation and recorded in the
ledger. Required properties, all satisfied:

- opaque and **non-semantic** (the number carries no meaning);
- issued **once** upon confirmation; **persisted** in the ledger;
- **never recomputed** from title, filename, membership, or source hashes;
- adding another manifestation to a song does **not** change its `SongId`;
- a rename or corrected title does **not** change it.

A **title-derived slug** and a **hash of the current membership set** are both
**rejected** as canonical identities (both violate stability under
rename / added-manifestation). The encoding is filesystem-safe and JSON-stable.

> The monotonic counter assumes a **single authoritative ledger**. If concurrent
> issuance across branches ever matters, a random **ULID/UUID** token is the
> drop-in alternative (still opaque, still ledger-recorded, never recomputed).
> Left as an open question for review; v1 assumes one ledger.

## 8. Versioned artifacts (v1 schemas)

All artifacts carry `schema`, `policy_id`, `policy_version` where applicable and
refer to sources by **exact `sha256`**. JSON, deterministically rendered.

### 8.1 Suggestion artifact

```json
{
  "schema": "song-curation.suggestions.v1",
  "policy_id": "metadata-only",
  "policy_version": "1",
  "corpus_fingerprint": "<hex>",
  "sources": [ { "source_sha256": "…", "filenames": ["…"], "titles": ["…"] } ],
  "suggested_groups": [
    { "candidate_id": "g1", "source_sha256s": ["…","…"], "confidence": "low|medium|high" }
  ],
  "evidence": [
    { "candidate_id": "g1", "signals": ["normalized_title_match", "filename_stem_match"], "note": "…" }
  ],
  "warnings": ["artist parsed from title is heuristic, not structured provenance"]
}
```

Every suggested group references exact source hashes; the artifact **never**
contains a `song_id`.

### 8.2 Decisions artifact (the ledger)

```json
{
  "schema": "song-curation.decisions.v1",
  "corpus_fingerprint": "<hex>",
  "curator": "…",
  "decided_at": "2026-07-29T00:00:00Z",
  "next_song_seq": 43,
  "decisions": [
    {
      "song_id": "song-000042",
      "source_sha256s": ["…","…"],
      "action": "define | accept | merge | split | correct",
      "note": "optional"
    }
  ]
}
```

- `next_song_seq` persists the issuance counter (§7).
- **Correction / merge semantics (no invisible rewrites):** a `correct` or
  `merge` decision is a **new, appended** decision referencing the prior
  `song_id`(s) it supersedes; earlier decisions are **never** edited in place.
  Apply replays decisions in order; the latest decision for a `sha256` wins, and
  a label change from a non-`None` value requires an explicit `correct`
  (`ExistingLabelReplacementNotAuthorized` otherwise). The ledger is thus an
  append-only audit trail.

### 8.3 Application report (evidence, not authority)

```json
{
  "schema": "song-curation.apply-report.v1",
  "input_corpus_fingerprint": "<hex>",
  "decisions_digest": "<hex>",
  "output_corpus_fingerprint": "<hex>",
  "assignments_applied": 0,
  "assignments_unchanged": 0,
  "coverage": { "unique_sources": 0, "labelled": 0, "unlabelled": 0, "songs": 0 },
  "refusals": [ { "kind": "…", "source_sha256": "…" } ],
  "holdout_ready": false
}
```

The report is **evidence**, never a second authority for `song_id`.

## 9. Determinism and the corpus fingerprint

**Selected algorithm.** The corpus fingerprint is `source_sha256` (griff-core's
lowercase-hex SHA-256) of a canonical blob built as follows, so directory
timestamps and traversal order cannot affect it:

1. For every chunk: the line `chunk\t<chunk_id>\t<source_sha256 or "">\t<song_id or "">\n`.
2. For every `CorpusManifest.songs` entry: the line
   `song\t<song_id>\t<sorted,comma-joined sha256 list>\n`.
3. Sort **all** lines as UTF-8 byte strings; concatenate; hash.

This detects **added/removed chunks**, **changed source hashes**, **changed
existing labels**, and **changed manifest membership** — exactly the drifts that
must invalidate a plan. Suggestion and plan output are **byte-deterministic** for
the same inputs and policy version.

## 10. Refusal taxonomy (typed, defined before implementation)

- `UnidentifiedSource` — a chunk / source has no `sha256`.
- `ConflictingExistingSongIds` — one `sha256` carries more than one `song_id`.
- `UnknownDecisionSource` — a decision names a `sha256` absent from the corpus.
- `SourceAssignedToMultipleSongs` — one `sha256` assigned to two `SongId`s.
- `DecisionCorpusFingerprintMismatch` — decisions were made against a different
  snapshot.
- `PlanCorpusFingerprintMismatch` — the corpus changed between plan and apply.
- `ExistingLabelReplacementNotAuthorized` — a non-`None` label would change
  without an explicit `correct` decision.
- `ManifestDisagreement` — `CorpusManifest.songs` disagrees with the per-source
  labels (propagated from the core preflight where applicable).
- `IncompleteCoverage` — not every participating source is labelled.
- `OutputWouldModifyInput` — the output path resolves inside/equal to an input.
- `OutputAlreadyExists` — the output path already exists.

`IncompleteCoverage` is a **validation status** during partial curation, but
becomes a **refusal** when `--require-holdout-ready` (or equivalent) is set.

## 11. Implementation sequence after acceptance (separate RED→GREEN slices)

1. **Decision & validation core** — parse/validate a human-authored decisions
   artifact; inventory sources by `sha256`; construct a deterministic **dry-run**
   plan; **no** suggestions; **no** corpus writes.
2. **Transactional application** — apply a validated plan to a **fresh** output
   tree; update every chunk of each source; generate the deterministic `songs`
   map; produce the application report; prove **idempotence** and **no partial
   writes**.
3. **Suggestion generator** — deterministic **metadata-only** suggestions;
   evidence-rich output; **no** write path and **no** implicit acceptance.
4. **Controlled corpus pilot** — only after **independent acceptance** of slices
   1–3; operate on a small **copied subset**; human-confirm every assignment;
   verify snapshot and manifest; run `song_holdout_preflight`; **no full-corpus
   labeling** until the pilot is independently accepted.

## 12. Explicit non-goals

Automatic song assignment; musical similarity or cover detection; embeddings,
classifiers, LLM decisions, or audio analysis; production scoring; generation
changes; cockpit integration; silent repair of existing inconsistent labels;
in-place mutation; full-corpus labeling; track-index recovery; arrangement
identity; any change to ADR-0031 or ADR-0032.

## 13. Governance path

This proposal PR is **discussion only**. After independent review:

1. accepted durable decisions move into a **new ADR (provisionally ADR-0033)**;
2. this proposal becomes historical context;
3. implementation starts **from the accepted ADR** as the separate RED→GREEN
   slices above;
4. **corpus labeling remains prohibited** until the implementation and
   controlled-pilot gates are explicitly opened.

## 14. Selected choices and open questions (for the reviewer)

**Selected in this draft (not left to implementation):**

- **Owner:** a standalone isolated `song-curation/` tool (§4), over extending
  `griff curate` / `griff manifest`.
- **Manifest strategy:** first implementation = tool exclusively owns song-aware
  manifest generation (strategy 1); durable end state = extend `griff manifest`
  (strategy 2, later, separately accepted). §5.6.
- **`SongId` encoding:** opaque ledger-issued `song-<zero-padded monotonic
  counter>` (§7).
- **Fingerprint:** SHA-256 over sorted domain-tagged chunk + songs lines (§9).
- **Suggestion policy v1:** metadata-only, evidence-bearing, artist-as-heuristic
  (§5.2).

**Left open for review:**

- Monotonic counter vs ULID/UUID for `SongId` under possible concurrent
  issuance (§7).
- Interim manifest-erasure guard: documentation (strategy 1) vs a hard
  refuse-to-overwrite at a distinct path (strategy 3) until strategy 2 lands
  (§5.6).
- Whether the decisions ledger is one file or a JSONL append log (both satisfy
  append-only; the schema in §8.2 is shown as a single document).
- The exact `strip_version_suffix` reuse vs a curation-specific normalization
  rule for suggestions (§5.2).
