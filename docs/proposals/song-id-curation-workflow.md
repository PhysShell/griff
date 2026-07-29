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
run must **not** convert suggestions into decisions. Confirmation is captured as
**append-only events** in the decisions ledger (§8.2), each naming its curator,
timestamp, corpus fingerprint, the exact `source_sha256`s, and — for a reviewed
suggestion — the `candidate_id`, so a later apply run can prove **every stored
label came from an explicit curator decision**, and "reviewed and rejected"
stays distinguishable from "never reviewed".

### 5.4 Plan (immutable)

Build an **immutable, serialized application plan** (§8.3) from:

- an exact **corpus fingerprint** (§9);
- a **versioned decisions ledger** (its digest);
- the **tool-policy version**.

The plan enumerates **every** intended source-level assignment and **every**
affected chunk, and carries a `plan_digest`. If the corpus fingerprint has
changed since inventory/confirm, planning or application **typed-refuses**
(`DecisionCorpusFingerprintMismatch` / `PlanCorpusFingerprintMismatch`) rather
than rebasing decisions onto a different snapshot.

### 5.5 Apply (transactional)

Apply **only** a previously validated plan. Before writing, Apply **recomputes
and verifies**: (1) the `plan_digest`; (2) the current corpus fingerprint against
the plan's `input_corpus_fingerprint`; (3) the `decisions_digest`; (4) every
source→chunk binding; (5) every `expected_existing_song_id`. Then:

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
  `UnknownDecisionSource` / `PlanDigestMismatch` typed-refuse;
- **never clear** an existing label unless an explicit curator **correction**
  authorizes it (`ExistingLabelReplacementNotAuthorized` otherwise; the plan's
  `expected_existing_song_id` is how the authorization is checked).

A **correction is a new curator decision**, not heuristic reconciliation.

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
  overwrite** the curated artifact (they live at different paths);
- **later (strategy 2, separately accepted):** teach `griff manifest` to rebuild
  `songs` from the authoritative per-source labels, at which point the distinct
  path can be retired.

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

## 6. Partial curation

Incremental curation is **supported** — requiring 100% completion before the
first confirmed decision would guarantee nobody ever curates. But:

- partial output is explicitly marked **`holdout_ready: false`**;
- it is **not** described as a valid song-holdout corpus;
- uncurated sources remain `song_id: None`;
- a partial run **never** invents an "unknown" shared `SongId`;
- the complete gate remains `song_holdout_preflight == Ok(())`.

Two states are **distinct** and must never be conflated: a **valid partial
curation snapshot** (`holdout_ready: false`, some sources still `None`) and a
**complete holdout-ready corpus** (`holdout_ready: true`, strict preflight passes).

## 7. `SongId` issuance (selected, closed for v1)

An **opaque, ledger-issued identifier** — `song-` + a zero-padded monotonic
counter maintained in the decisions ledger (e.g. `song-000042`) — issued **once**
at human confirmation and recorded in the ledger. Properties, all satisfied:

- opaque and **non-semantic** (the number carries no meaning);
- issued **once** upon confirmation; **persisted** in the ledger;
- **never recomputed** from title, filename, membership, or source hashes;
- adding another manifestation to a song does **not** change its `SongId`;
- a rename or corrected title does **not** change it.

A **title-derived slug** and a **hash of the current membership set** are both
**rejected** as canonical identities. The encoding is filesystem-safe and
JSON-stable. **v1 assumes a single authoritative single-writer ledger; concurrent
issuance across writers is explicitly out of scope for v1** (a random ULID/UUID
would be the drop-in encoding if that ever changes — a future policy-version bump,
not an open v1 alternative).

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

### 8.2 Decisions ledger — append-only per-event, following `CurationStoreV1`

One versioned JSON document (not JSONL) with an **append-only `events` array**,
matching the `core/src/curation_store.rs` `CurationStoreV1` / `CurationEvent`
precedent — where each event carries its **own** `event_id`, `curator`,
`occurred_at`, and `corpus_fingerprint`. Per-event fingerprints are load-bearing:
after a partial apply the corpus fingerprint changes, so a later event must bind
to **its own** snapshot; an envelope-only fingerprint could not be appended to
honestly (it would either misbind the new event or retroactively rebind old ones).

```json
{
  "schema": "song-curation.decisions.v1",
  "created_corpus_fingerprint": "<hex>",
  "next_song_seq": 43,
  "events": [
    {
      "event_id": "ev-000017",
      "curator": "…",
      "occurred_at": "2026-07-29T00:00:00Z",
      "corpus_fingerprint": "<hex, this event's snapshot>",
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
```

The envelope's `created_corpus_fingerprint` is informational (the store's first
snapshot); the **authoritative** fingerprint for each decision is the one on its
event. `next_song_seq` persists the issuance counter (§7).

**`action` is a tagged union** (not one string plus fields meaningless for half
the variants). Every variant carries the resulting `source_sha256s` (exact),
`supersedes_song_ids` (possibly empty), and the assignment(s) it produces:

- `accept_suggestion { candidate_id, source_sha256s, assign_song_id, supersedes_song_ids }`
- `reject_suggestion { candidate_id, reviewed_source_sha256s, reason? }` — records
  a review that produced **no** assignment, so it stays distinct from "never
  reviewed";
- `manual_define { source_sha256s, assign_song_id }`
- `split { from_song_id, into: [ { assign_song_id, source_sha256s } … ], supersedes_song_ids: [from_song_id] }`
- `merge { from_song_ids, into_song_id, source_sha256s, supersedes_song_ids: from_song_ids }`
- `correct { source_sha256s, new_song_id, supersedes_song_ids }` — the only
  variant permitted to change a non-`None` label.

**No invisible rewrites:** events are immutable and append-only; a `correct` /
`merge` / `split` is a **new** event referencing the `song_id`(s) it supersedes.
Apply replays events in order; the latest event for a `sha256` wins; a change from
a non-`None` label requires a `correct` (`ExistingLabelReplacementNotAuthorized`
otherwise).

### 8.3 Plan artifact (the only thing Apply consumes)

```json
{
  "schema": "song-curation.plan.v1",
  "policy_id": "…",
  "policy_version": "1",
  "input_corpus_fingerprint": "<hex>",
  "decisions_digest": "<hex>",
  "plan_digest": "<hex>",
  "assignments": [
    {
      "source_sha256": "…",
      "song_id": "song-000042",
      "expected_existing_song_id": null,
      "affected_chunk_ids": ["…","…"]
    }
  ],
  "generated_songs_map": { "song-000042": ["<sorted-sha256>", "…"] }
}
```

`plan_digest` is computed over the canonical plan bytes **with the `plan_digest`
field omitted** (§9 record encoding). Apply verifies it (blocker: "apply only a
validated plan" is otherwise decorative). `expected_existing_song_id` is how a
label change is authorized: Apply refuses if the on-disk label differs from it.

### 8.4 Application report (evidence, not authority)

```json
{
  "schema": "song-curation.apply-report.v1",
  "input_corpus_fingerprint": "<hex>",
  "decisions_digest": "<hex>",
  "plan_digest": "<hex>",
  "output_corpus_fingerprint": "<hex>",
  "curated_manifest_path": "song-curation/manifest.json",
  "curated_manifest_digest": "<hex>",
  "assignments_applied": 0,
  "assignments_unchanged": 0,
  "coverage": { "unique_sources": 0, "labelled": 0, "unlabelled": 0, "songs": 0 },
  "refusals": [ { "kind": "…", "source_sha256": "…" } ],
  "holdout_ready": false
}
```

The report is **evidence**, never a second authority for `song_id`.

## 9. Determinism and the corpus fingerprint (injective)

The core already has `corpus_fingerprint()` (`core/src/curation_store.rs:228`),
but it deliberately hashes each chunk's **material** identity and excludes mutable
curation fields — so it cannot detect a `song_id` or manifest-membership change,
which is exactly what a curation plan must be invalidated by. This workflow
therefore defines its **own** fingerprint over the label-bearing inputs.

**Encoding (injective, canonical).** Build these records, one per item, as
**compact UTF-8 JSON arrays** (JSON escaping makes the encoding injective —
`ChunkId` and `SongId` are unrestricted `String` and may contain tabs or
newlines, which an ad-hoc separator could not survive):

```json
["manifest_songs", "absent"]            // when CorpusManifest.songs is None
["manifest_songs", "present"]           // when Some(...) — even empty {}
["chunk", "<chunk_id>", "<sha256-or-null>", "<song_id-or-null>"]
["song", "<song_id>", ["<sorted-sha256>", "…"]]   // one per songs-map entry
```

Sort the record **bytes** as UTF-8 strings, join with `\n`, and `source_sha256`
the result. This detects added/removed chunks, changed source hashes, changed
existing labels, and changed manifest membership; directory timestamps and
traversal order cannot affect it; and the `manifest_songs` presence record
distinguishes **absent** (`None`, skips cross-check) from **present-empty**
(`Some({})`, must account for every labelled source) — a distinction core makes
deliberately. Suggestion, plan, and fingerprint output are byte-deterministic for
the same inputs and policy version. The same compact-JSON-record + sort + hash
scheme defines `plan_digest` and `decisions_digest`.

## 10. Refusal taxonomy (typed, defined before implementation)

- `UnidentifiedSource` — a chunk / source has no `sha256`.
- `ConflictingExistingSongIds` — one `sha256` carries more than one `song_id`.
- `UnknownDecisionSource` — a decision names a `sha256` absent from the corpus.
- `SourceAssignedToMultipleSongs` — one `sha256` assigned to two `SongId`s.
- `DecisionCorpusFingerprintMismatch` — decisions were made against a different
  snapshot (per-event fingerprint mismatch).
- `PlanCorpusFingerprintMismatch` — the corpus changed between plan and apply.
- `PlanDigestMismatch` — the plan bytes do not match `plan_digest`.
- `ExistingLabelReplacementNotAuthorized` — a non-`None` label would change
  without an explicit `correct` (checked via `expected_existing_song_id`).
- `ManifestDisagreement` — `CorpusManifest.songs` disagrees with the per-source
  labels (propagated from the core preflight where applicable).
- `IncompleteCoverage` — not every participating source is labelled.
- `OutputWouldModifyInput` — the output path resolves inside/equal to an input.
- `OutputAlreadyExists` — the output path already exists.

`IncompleteCoverage` is a **validation status** during partial curation, but
becomes a **refusal** when `--require-holdout-ready` (or equivalent) is set.

## 11. Implementation sequence after acceptance (separate RED→GREEN slices)

1. **Decision & validation core** — parse/validate a human-authored decisions
   ledger; inventory sources by `sha256`; construct the deterministic serialized
   **dry-run plan** (§8.3) and verify its digest; **no** suggestions; **no**
   corpus writes.
2. **Transactional application** — apply a validated plan to a **fresh** output
   tree; update every chunk of each source; generate the deterministic `songs`
   map at the distinct curated path; produce the application report; prove
   **idempotence** and **no partial writes**.
3. **Suggestion generator** — deterministic **metadata-only** suggestions;
   evidence-rich output; **no** write path and **no** implicit acceptance.
4. **Controlled corpus pilot** — only after **independent acceptance** of slices
   1–3; operate on a small **copied subset**; human-confirm every assignment;
   verify snapshot and the curated manifest by digest; run
   `song_holdout_preflight`; **no full-corpus labeling** until the pilot is
   independently accepted.

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
- **Ledger format:** one versioned JSON document with immutable, append-only
  **per-event** records following `CurationStoreV1` (§8.2) — **not** JSONL, **not**
  an envelope-only fingerprint.
- **Action model:** a **tagged `action` union** covering accept / reject / manual
  define / split / merge / correct, each with exact hashes and
  `supersedes_song_ids` (§8.2).
- **Plan:** a serialized, digest-verified `song-curation.plan.v1` artifact is the
  **only** thing Apply consumes (§8.3, §5.5).
- **`SongId` encoding:** opaque ledger-issued `song-<zero-padded monotonic
  counter>`, single-writer ledger; concurrent issuance **out of scope for v1**
  (§7).
- **Fingerprint / digests:** SHA-256 over sorted **compact-JSON records** with a
  `manifest_songs` absent/present marker (§9) — injective, presence-aware; the
  same scheme defines `plan_digest` and `decisions_digest`.
- **Manifest guard:** the tool writes to a **distinct curated path** and refuses
  ordinary `<corpus>/manifest.json`; ordinary `griff manifest` cannot overwrite
  it; strategy 2 (extend `griff manifest`) is a later, separately-accepted change
  (§5.6).
- **Suggestion normalization:** reuse the census `strip_version_suffix` rule
  **exactly** for v1; any later divergence requires a policy-version bump (§5.2).

Genuinely deferred (out of scope for v1, **not** unresolved alternatives):
multi-writer/concurrent `SongId` issuance; the strategy-2 `griff manifest`
extension; any non-metadata suggestion signal.
