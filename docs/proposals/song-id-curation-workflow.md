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

```text
inventory sources
  → generate non-authoritative grouping suggestions
  → record explicit curator decisions (in append-only batches)
  → build a deterministic application plan that embeds one unapplied batch
  → apply that plan transactionally to a fresh corpus copy
  → generate the songs manifest
  → validate consistency and holdout readiness
```

The system reduces curation **labour**; it never lets a heuristic promote itself
to a stored identity, and the ledger → plan → apply → report forms **one
verifiable chain**, not three documents trading meaningful-looking hashes.

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
  (`ChunkMeta`, `SourceRef`, `CorpusManifest`, `SongId`, `source_sha256`, the
  existing `song_holdout_preflight`, and the `CurationStoreV1` event/ledger
  precedent in `core/src/curation_store.rs`).

### Comparison (required)

| Owner | For | Against | Verdict |
|---|---|---|---|
| **Standalone `song-curation/`** (selected) | Matches the isolated-tool precedent; a multi-phase ledger/plan/apply workflow is too large for a subcommand; no production surface acquires curation policy; carries its own artifact schemas. | One more isolated crate; not exercised by workspace CI (per the isolation policy, verified locally). | **Selected.** |
| Extend `griff curate` | Reuses an existing curation entry point. | `griff curate` is interactive per-chunk cockpit-adjacent curation; song identity is a *source-level, ledgered, transactional* operation with a fingerprint/plan/apply contract that does not fit an interactive per-chunk command, and it would pull curation policy into a production binary. | Rejected. |
| Extend `griff manifest` | Manifest generation already emits `CorpusManifest`. | `griff manifest` folds chunks into a manifest; it is a pure projection, not a curation ledger. Owning issuance/decisions there overloads it and adds policy to production. | Rejected for *ownership* (but see §5.6 for its eventual manifest role). |

Ownership is **selected now**, not deferred to implementation.

## 5. Workflow phases

### 5.1 Inventory (read-only)

Read a corpus snapshot and **collapse chunks by exact source `sha256`** into
deterministic source records with at least:

```text
source_sha256
filenames                     # sorted, unique
titles                        # sorted, unique
formats                       # sorted, unique
chunk_ids                     # sorted
existing_song_ids             # sorted, unique (from SourceRef.song_id)
existing_manifest_membership  # SongIds naming this sha256 in CorpusManifest.songs
```

A source with **more than one** distinct existing `song_id` across its chunks →
`ConflictingExistingSongIds`. A chunk without `sha256` → `UnidentifiedSource`.
Inventory writes nothing.

### 5.2 Suggest (non-authoritative)

Deterministic, **evidence-bearing** candidate groupings. **Version 1 uses
metadata evidence only:**

- normalized `ChunkMeta.title`;
- normalized source filenames / stems;
- repeated source names with format/version suffixes removed by the **census
  `strip_version_suffix` rule, reused exactly** (trailing `(...)` removed only
  when the inner text starts with `ver`, contains ` by `, or is all-digits —
  never for e.g. `(Reprise)`);
- already-confirmed identity relationships, when supplied.

**No canonical artist field exists in the schema.** "Artist" parsed from a title
or filename is a **suggestion heuristic**, reported as such in `evidence`; it is
not structured provenance. A structured artist signal, if ever wanted, must
arrive as a **separate explicit input artifact**, not inferred.

**No** note-content similarity, embeddings, audio fingerprinting, cover
detection, or MIR classifier enters version 1. Every suggested group exposes its
**evidence** and **uncertainty**, refers to exact source hashes, and **never
writes `song_id`**.

### 5.3 Confirm (explicit)

A curator explicitly **accepts / rejects / splits / merges / manually defines**
source groupings. **No default action means acceptance.** A batch or unattended
run must **not** convert suggestions into decisions. Confirmation appends
immutable **events** to the current **decision batch** (§8.2); every event names
its curator, timestamp, exact `source_sha256`s, and — for a reviewed suggestion —
the `candidate_id`, so a later apply run can prove **every stored label came from
an explicit curator decision**, and "reviewed and rejected" stays distinct from
"never reviewed".

### 5.4 Plan (immutable, embeds one unapplied batch)

Build an **immutable, serialized application plan** (§8.3) that **embeds exactly
one unapplied decision batch** — the complete ordered event objects, not just a
digest — so that Apply's decisions come entirely from the plan. The plan also
carries:

- the batch's `input_corpus_fingerprint` (§9);
- `decisions_digest` — the digest of the embedded, **order-preserving** batch;
- the batch's `previous_application_report_digest` (chaining, §8.4);
- the **derived** `assignments` (every source-level `SongId` and every affected
  chunk) and `generated_songs_map`;
- `plan_digest` over the whole plan.

Planning refuses if the batch's `input_corpus_fingerprint` does not match the
current corpus (`DecisionBatchFingerprintMismatch`), or if the events within one
batch do not all share that fingerprint.

### 5.5 Apply (transactional, chain-verified)

Apply consumes the **plan** (which embeds its decision batch — the sole source of
the *decisions* it applies) and the **application index** (§8.5) — the append-only
registry of already-applied batches that makes the chain and already-applied
checks *provable* from a declared input, not inferred from corpus side-effects.
Before writing, Apply:

1. recomputes `plan_digest` over the plan (`PlanDigestMismatch` on mismatch);
2. recomputes the embedded batch's `decisions_digest` and checks it equals the
   plan's `decisions_digest` field (`DecisionDigestMismatch`);
3. verifies the batch `input_corpus_fingerprint` equals the **current** corpus
   fingerprint (`PlanCorpusFingerprintMismatch`);
4. verifies the application chain against the index: the batch's `batch_id` is
   **absent** from the index (`DecisionBatchAlreadyApplied`); and — for a
   non-initial batch — the batch's `previous_application_report_digest` equals the
   index's last `report_digest`, whose `output_corpus_fingerprint` equals this
   batch's `input_corpus_fingerprint` (`ApplicationChainMismatch`);
5. **replays the embedded events itself**, derives assignments +
   `generated_songs_map`, and compares them to the plan's — any divergence is
   `DecisionProjectionMismatch` (the plan cannot assert an assignment the
   decisions do not produce);
6. checks every `expected_existing_song_id` against the on-disk label, and that
   each label change is covered by the acting event's authorized supersession set
   (§8.2).

Only then does it write:

- to a **fresh output directory** (`OutputAlreadyExists` / `OutputWouldModifyInput`,
  reusing the `migrate-v9` preflight discipline);
- **all-or-nothing**; no partial output presented as successful;
- updating **every** chunk sharing an assigned `sha256`;
- preserving every unrelated field byte-for-byte (only `SourceRef.song_id`
  changes);
- **deterministic** file order and JSON rendering; **idempotent** for
  already-correct labels;
- **never** clearing a non-`None` label unless the acting event's supersession
  set authorizes it — `correct`, `merge`, or `split` (§8.2)
  (`ExistingLabelReplacementNotAuthorized`; the plan's `expected_existing_song_id`
  is the check).

On success Apply publishes the application report **and** appends a matching
record to the application index **transactionally** — both land or neither does,
so the registry never lies about what was applied. A **correction / merge / split
is a new curator decision**, not heuristic reconciliation.

### 5.6 Generate manifest (deterministic) — hard distinct-path guard

Generate `CorpusManifest.songs` deterministically from the applied per-source
labels: `SongId → sorted, unique [sha256]`. The per-source `SourceRef.song_id`
remains **authoritative**; the map is the cross-check (law 8).

**Selected interim contract (hard, not documentation).** Ordinary `griff
manifest` today always emits `songs: None` — the command builder in
`cli/src/main.rs:1998` constructs `CorpusManifest { …, songs: None }` (its shared
seam is `ui-core/src/corpus.rs:18`). A warning is not a data-integrity mechanism,
so the first implementation enforces a hard guard:

- the `song-curation/` tool writes the curated manifest to a **distinct canonical
  path** (e.g. `song-curation/manifest.json`);
- it **refuses** to target an ordinary `<corpus>/manifest.json`;
- the **application report** records the curated manifest path and its digest;
- the controlled pilot (§11) consumes **that explicit path and digest**;
- ordinary `griff manifest` may create its normal manifest, but **cannot
  overwrite** the curated artifact (different paths);
- **later (strategy 2, separately accepted):** teach `griff manifest` to rebuild
  `songs` from the authoritative per-source labels, retiring the distinct path.

This is fail-closed: no ordinary manifest rebuild can silently erase the curated
cross-check, because it never writes to the curated path.

### 5.7 Validate

Validation **uses the existing core `song_holdout_preflight`**, not a near-copy.
It also reports:

```text
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

## 6. Partial curation — the batch/apply chain

Incremental curation is **first-class**: it proceeds as a **chain of batches**,
each applied to the corpus the previous one produced. After two partial passes a
ledger holds a batch against pristine fingerprint `A` (apply → corpus `B`,
report `R_A`) and a later batch against `B` (`previous_application_report_digest
= digest(R_A)`). This resolves the "which events belong to this application"
question the flat-replay model could not:

- every event in one batch binds to the **same** `input_corpus_fingerprint`;
- **one plan embeds exactly one unapplied batch**;
- **historical batches are audit history and are never replayed**;
- the **application index** (§8.5) is the append-only registry of applied
  batches; the next batch's `input_corpus_fingerprint` must equal the index's
  last `output_corpus_fingerprint`, and its `previous_application_report_digest`
  the index's last `report_digest` (`ApplicationChainMismatch` otherwise);
- reuse of a batch whose `batch_id` is already in the index typed-refuses
  (`DecisionBatchAlreadyApplied`) — the registry makes this **provable** rather
  than inferred from corpus side-effects, so it also catches a rejection-only or
  otherwise fingerprint-neutral batch, and re-applying an initial batch to a
  fresh copy of the same snapshot.

Partial output is `holdout_ready: false`, is **not** a valid song-holdout corpus,
leaves uncurated sources `song_id: None`, never invents an "unknown" shared
`SongId`, and the complete gate remains `song_holdout_preflight == Ok(())`. A
**valid partial snapshot** and a **complete holdout-ready corpus** are distinct
states, never conflated.

## 7. `SongId` issuance (selected, closed for v1)

An **opaque, ledger-issued identifier** — `song-` + a zero-padded monotonic
counter maintained in the decisions ledger (e.g. `song-000042`) — issued **once**
at human confirmation and recorded in the ledger. Properties, all satisfied:

- opaque and **non-semantic**;
- issued **once** upon confirmation; **persisted** in the ledger;
- **never recomputed** from title, filename, membership, or source hashes;
- adding another manifestation does **not** change its `SongId`; a rename or
  corrected title does **not** change it.

A **title-derived slug** and a **hash of the current membership set** are both
**rejected**. The encoding is filesystem-safe and JSON-stable. **v1 assumes a
single authoritative single-writer ledger; concurrent issuance is explicitly out
of scope for v1** (a random ULID/UUID would be the drop-in encoding via a future
policy-version bump — not an open v1 alternative).

## 8. Versioned artifacts (v1 schemas)

All artifacts are deterministically-rendered JSON, refer to sources by exact
`sha256`, and carry `schema` (and `policy_id` / `policy_version` where a policy
is involved).

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
    { "candidate_id": "g1", "signals": ["normalized_title_match","filename_stem_match"], "note": "…" }
  ],
  "warnings": ["artist parsed from title is heuristic, not structured provenance"]
}
```

Every suggested group references exact source hashes; the artifact **never**
contains a `song_id`.

### 8.2 Decisions ledger — append-only **batches** of immutable events

One versioned JSON document (not JSONL) following the `CurationStoreV1`
event/ledger precedent (`core/src/curation_store.rs`). The ledger is an ordered
list of immutable **batches**; each batch is the unit of one application and
binds all its events to a single `input_corpus_fingerprint`:

```json
{
  "schema": "song-curation.decisions.v1",
  "next_song_seq": 43,
  "batches": [
    {
      "batch_id": "batch-000004",
      "input_corpus_fingerprint": "<hex>",
      "previous_application_report_digest": null,
      "events": [
        {
          "event_id": "ev-000017",
          "ordinal": 0,
          "curator": "…",
          "occurred_at": "2026-07-29T00:00:00Z",
          "note": "optional",
          "action": {
            "kind": "accept_suggestion",
            "candidate_id": "g1",
            "source_sha256s": ["…","…"],
            "assign_song_id": "song-000042",
            "supersedes_song_ids": []
          }
        }
      ]
    }
  ]
}
```

- `next_song_seq` persists the issuance counter (§7).
- A batch's `input_corpus_fingerprint` is the fingerprint **all** its events were
  made against; `previous_application_report_digest` chains it to the preceding
  accepted application report (`null` for the first batch).
- **`action` is a tagged union**; each variant carries exact `source_sha256s`,
  `supersedes_song_ids` (possibly empty), and its assignment(s):
  - `accept_suggestion { candidate_id, source_sha256s, assign_song_id, supersedes_song_ids }`
  - `reject_suggestion { candidate_id, reviewed_source_sha256s, reason? }` — a
    review that produced **no** assignment (distinct from "never reviewed");
  - `manual_define { source_sha256s, assign_song_id }`
  - `split { from_song_id, into: [ { assign_song_id, source_sha256s } … ], supersedes_song_ids: [from_song_id] }`
  - `merge { from_song_ids, into_song_id, source_sha256s, supersedes_song_ids: from_song_ids }`
  - `correct { source_sha256s, new_song_id, supersedes_song_ids }`.
- **Authorized replacement (three actions, not one).** Changing a non-`None`
  existing label is permitted by exactly the action whose supersession set covers
  it:
  - `correct` — may replace exactly the labels named in its `supersedes_song_ids`;
  - `merge` — may replace labels from `from_song_ids` with `into_song_id`;
  - `split` — may replace `from_song_id` with the explicitly enumerated target
    assignments.

  `ExistingLabelReplacementNotAuthorized` fires when the actual on-disk label is
  **not** in the authorized supersession set of the acting event
  (`accept_suggestion` / `manual_define` carry an empty set, so they may only
  label `None`; `reject_suggestion` assigns nothing). All six actions are
  first-class.
- **Ordering — one source of truth.** The `events` **array order is the
  authoritative append order**; `ordinal` MUST equal the zero-based array
  position; ordinals MUST be contiguous and unique; `event_id` MUST be unique
  within the ledger. Any violation typed-refuses (`InvalidDecisionBatchOrder` /
  `DuplicateDecisionEventId`) **before** any digest or replay. Within one batch,
  the **latest event (highest ordinal) for a `sha256` wins**.
- **No invisible rewrites:** events (and applied batches) are immutable and
  append-only; historical batches are audit history and are **never** replayed.

### 8.3 Plan artifact — embeds one unapplied batch (Apply's decision source)

```json
{
  "schema": "song-curation.plan.v1",
  "policy_id": "…",
  "policy_version": "1",
  "input_corpus_fingerprint": "<hex>",
  "decision_batch": {
    "batch_id": "batch-000004",
    "input_corpus_fingerprint": "<hex>",
    "previous_application_report_digest": null,
    "events": [ "…the complete ordered event objects…" ]
  },
  "decisions_digest": "<order-preserving digest of decision_batch>",
  "plan_digest": "<hex>",
  "assignments": [
    { "source_sha256": "…", "song_id": "song-000042", "expected_existing_song_id": null, "affected_chunk_ids": ["…","…"] }
  ],
  "generated_songs_map": { "song-000042": ["<sorted-sha256>", "…"] }
}
```

The plan **embeds the complete ordered batch**, so Apply verifies
`decisions_digest`, **replays the events, derives the assignments itself, and
compares** them to `assignments` / `generated_songs_map` (§5.5) — a digest cannot
be recomputed from its own field, and `plan_digest` alone proves only internal
integrity, not derivation from the decisions. `expected_existing_song_id`
authorizes any label change.

### 8.4 Application report (evidence + chain link, not authority)

```json
{
  "schema": "song-curation.apply-report.v1",
  "report_digest": "<hex>",
  "batch_id": "batch-000004",
  "applied_event_ids": ["ev-000017", "…in order…"],
  "input_corpus_fingerprint": "<hex>",
  "output_corpus_fingerprint": "<hex>",
  "decisions_digest": "<hex>",
  "plan_digest": "<hex>",
  "curated_manifest_path": "song-curation/manifest.json",
  "curated_manifest_digest": "<hex>",
  "assignments_applied": 0,
  "assignments_unchanged": 0,
  "coverage": { "unique_sources": 0, "labelled": 0, "unlabelled": 0, "songs": 0 },
  "refusals": [ { "kind": "…", "source_sha256": "…" } ],
  "holdout_ready": false
}
```

`report_digest` (the report canonicalized per §9 with that field omitted) is what
the **next** batch's `previous_application_report_digest` references, closing the
chain. The report is **evidence and a chain link**, never a second authority for
`song_id`.

### 8.5 Application index — the append-only applied-batch registry

The registry that makes `DecisionBatchAlreadyApplied` and the chain checks
**provable from a declared Apply input**, rather than inferred from corpus
side-effects (which a rejection-only or fingerprint-neutral batch would evade):

```json
{
  "schema": "song-curation.applications.v1",
  "applications": [
    {
      "batch_id": "batch-000004",
      "report_digest": "<hex>",
      "input_corpus_fingerprint": "<hex>",
      "output_corpus_fingerprint": "<hex>"
    }
  ]
}
```

Plan creation and Apply check the batch's `batch_id` is **absent** here; Apply
appends the new record **transactionally with report publication** (both land or
neither). The last record is the chain head (§5.5 step 4).

## 9. Determinism — one canonical encoding, four ordering contracts

The core already has `corpus_fingerprint()` (`core/src/curation_store.rs:228`),
but it hashes each chunk's **material** identity and excludes mutable curation
fields, so it cannot detect a `song_id` or manifest-membership change — exactly
what a curation plan must be invalidated by. This workflow therefore defines its
own digests.

**Shared canonical JSON encoding.** All four contracts below serialize to
**compact UTF-8 JSON with object keys sorted lexicographically** and no
insignificant whitespace, then SHA-256. Fixed key ordering plus JSON escaping
(injective — `ChunkId` / `SongId` are unrestricted `String` and may contain
tabs/newlines an ad-hoc separator could not survive) makes the byte encoding
deterministic across implementations. What differs per contract is **which arrays
are sorted and which preserve semantic order**:

**1. `corpus_fingerprint` — order-insensitive (set-like).** One record per item;
**sort the record bytes**; join with `\n`:

```json
["manifest_songs", "absent"]            // CorpusManifest.songs == None
["manifest_songs", "present"]           // Some(...) — even empty {}
["chunk", "<chunk_id>", "<sha256-or-null>", "<song_id-or-null>"]
["song", "<song_id>", ["<sorted-sha256>", "…"]]
```

The `manifest_songs` record distinguishes **absent** (`None`, skips the
cross-check) from **present-empty** (`Some({})`, must account for every labelled
source) — a distinction core makes deliberately.

**2. `decisions_digest` — order-*sensitive*.** Canonicalize the decision **batch**
with its `events` array in **append order** (never sorted; `ordinal` equals the
array position, §8.2). Two ledgers with the same events in different order produce
different assignments ("latest wins"), so their digests **must** differ.

**3. `plan_digest` — order-aware.** Canonicalize the complete plan with the
`plan_digest` field omitted; the embedded `decision_batch` keeps its `events` in
append order (per contract 2); `assignments` sorted by `source_sha256`, each
`affected_chunk_ids` sorted, `generated_songs_map` keys and value lists sorted.

**4. `report_digest` — order-aware.** Canonicalize the complete application report
with the `report_digest` field omitted; `applied_event_ids` preserved in **batch
order**; `refusals` sorted by the fixed tuple `(kind, source_sha256)`. This
contract is load-bearing because the next batch's
`previous_application_report_digest` references it (§8.4, §8.5).

## 10. Refusal taxonomy (typed, defined before implementation)

- `UnidentifiedSource` — a chunk / source has no `sha256`.
- `ConflictingExistingSongIds` — one `sha256` carries more than one `song_id`.
- `UnknownDecisionSource` — a decision names a `sha256` absent from the corpus.
- `SourceAssignedToMultipleSongs` — one `sha256` assigned to two `SongId`s.
- `InvalidDecisionBatchOrder` — a batch's `ordinal`s are not contiguous, unique,
  and equal to the array position (§8.2).
- `DuplicateDecisionEventId` — an `event_id` repeats within the ledger.
- `DecisionBatchFingerprintMismatch` — a batch's `input_corpus_fingerprint` does
  not match the corpus it is planned/applied against (or its events disagree on
  it).
- `PlanCorpusFingerprintMismatch` — the corpus changed between plan and apply.
- `PlanDigestMismatch` — the canonical plan bytes do not match `plan_digest`.
- `DecisionDigestMismatch` — the embedded batch does not match the plan's
  `decisions_digest` field (distinct from the two above).
- `DecisionProjectionMismatch` — the batch is valid, but replaying it does not
  reproduce the plan's `assignments` / `generated_songs_map`.
- `DecisionBatchAlreadyApplied` — the batch's `batch_id` is already in the
  application index (§8.5).
- `ApplicationChainMismatch` — the batch's `previous_application_report_digest`
  or `input_corpus_fingerprint` does not chain to the index's last record.
- `ExistingLabelReplacementNotAuthorized` — a non-`None` label would change, but
  the actual on-disk label is **not** in the acting event's authorized
  supersession set (`correct` / `merge` / `split`, §8.2; checked via
  `expected_existing_song_id`).
- `ManifestDisagreement` — `CorpusManifest.songs` disagrees with the per-source
  labels (propagated from the core preflight where applicable).
- `IncompleteCoverage` — not every participating source is labelled.
- `OutputWouldModifyInput` — the output path resolves inside/equal to an input.
- `OutputAlreadyExists` — the output path already exists.

`IncompleteCoverage` is a **validation status** during partial curation, but
becomes a **refusal** when `--require-holdout-ready` (or equivalent) is set.

## 11. Implementation sequence after acceptance (separate RED→GREEN slices)

1. **Decision & validation core** — parse/validate the batched decisions ledger;
   inventory by `sha256`; construct the serialized dry-run plan (§8.3), embed the
   unapplied batch, and verify `plan_digest` / `decisions_digest` /
   `DecisionProjectionMismatch` by re-deriving assignments; **no** suggestions;
   **no** corpus writes.
2. **Transactional application** — apply a validated plan to a **fresh** output
   tree; verify the application chain; update every chunk of each source;
   generate the deterministic `songs` map at the distinct curated path; produce
   the chained application report; prove **idempotence** and **no partial writes**.
3. **Suggestion generator** — deterministic **metadata-only** suggestions;
   evidence-rich; **no** write path and **no** implicit acceptance.
4. **Controlled corpus pilot** — only after **independent acceptance** of slices
   1–3; a small **copied subset**; human-confirm every assignment; verify the
   snapshot and curated manifest by digest; run `song_holdout_preflight`; **no
   full-corpus labeling** until the pilot is independently accepted.

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

## 14. Closed v1 choices (no architecture lottery for the implementer)

Every choice below is **selected for v1**, not left open:

- **Owner:** standalone isolated `song-curation/` tool (§4).
- **Ledger:** one versioned JSON document of append-only **batches** of immutable
  events, following `CurationStoreV1` (§8.2) — not JSONL. Each batch binds its
  events to one `input_corpus_fingerprint` and is the unit of one application.
- **Action model:** a tagged `action` union (accept / reject / manual_define /
  split / merge / correct), all six first-class; `correct`, `merge`, and `split`
  are the **authorized replacement** actions, each replacing exactly the labels in
  its supersession set (§8.2).
- **Plan:** a serialized plan that **embeds exactly one unapplied batch**; Apply
  replays it, re-derives assignments, and compares (§8.3, §5.5) — the plan is the
  sole source of the *decisions*.
- **Application chain + index:** batch → report → next batch, linked by
  `previous_application_report_digest` and matching input/output fingerprints,
  with an append-only **application index** (§8.5) as the provable already-applied
  registry (an Apply input, written transactionally with the report).
- **`SongId` encoding:** opaque ledger-issued `song-<monotonic counter>`,
  single-writer; concurrent issuance out of scope for v1 (§7).
- **Digests:** one **shared canonical JSON encoding** (sorted object keys, compact
  UTF-8) under **four** ordering contracts (§9): order-insensitive
  `corpus_fingerprint`; order-**sensitive** `decisions_digest`; order-aware
  `plan_digest`; order-aware `report_digest`.
- **Manifest guard:** distinct curated path; refuse ordinary
  `<corpus>/manifest.json`; strategy 2 later (§5.6).
- **Suggestion normalization:** reuse the census `strip_version_suffix` rule
  **exactly** for v1; later divergence requires a policy-version bump (§5.2).

Genuinely deferred (out of scope for v1, **not** unresolved alternatives):
multi-writer/concurrent `SongId` issuance; the strategy-2 `griff manifest`
extension; any non-metadata suggestion signal.
