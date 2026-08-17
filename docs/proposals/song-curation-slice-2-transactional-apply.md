# ADR-0033 Slice 2 — transactional Apply: acceptance contract

Status: **proposed — awaiting independent acceptance**. ADR-0033 Decision 10
gates every slice after Slice 1 behind separate independent acceptance; this
document is the executable contract that a later RED→GREEN implementation of
Slice 2 must satisfy. **Implementation of Slice 2 remains prohibited until this
contract is independently accepted.** Nothing here reopens ADR-0033, ADR-0031,
or ADR-0032, and nothing here modifies the accepted, frozen Slice 1.

## 1. Authority and dependencies

Normative, in order of precedence:

1. [ADR-0033](../adr/0033-song-id-curation-workflow.md) — accepted, immutable.
   Decisions 5–9 bind Apply directly; Decision 10 is the gate this document
   passes through.
2. [ADR-0031](../adr/0031-canonical-song-identity.md) — `SongId` semantics;
   per-`SourceRef.song_id` is authoritative after application,
   `CorpusManifest.songs` is a convenience and cross-check.
3. [ADR-0032](../adr/0032-holdout-filtering-boundary.md) — the fail-closed
   holdout boundary this workflow ultimately serves.
4. The accepted **Slice 1** implementation —
   [`../../song-curation/`](../../song-curation/README.md)
   (`song-curation/src/lib.rs`): artifact schemas `song-curation.decisions.v1`
   and `song-curation.plan.v1`, the four digest contracts, `verify_plan`, and
   the Slice-1 refusal set. Frozen; consumed, not reinterpreted.
5. Core contracts: `core/src/corpus.rs` (`ChunkMeta`, `SourceRef`,
   `CorpusManifest`, `song_holdout_preflight`) and the isolation posture of
   ADR-0010.

Historical context only (not a live contract): the
[song-id curation workflow proposal](song-id-curation-workflow.md). Where this
document uses one of its detailed rules that ADR-0033 does not already fix, the
rule is **explicitly adopted, modified, or rejected** in §13 — nothing is
inherited by silence.

Precedent consumed: the `migrate-v9` output preflight
(`migrate/src/main.rs::preflight_output`) and its fail-closed
validate-everything-before-writing-anything discipline.

## 2. Goal and scope

Slice 2 is the smallest complete **transactional Apply boundary**: it consumes
a verified Slice-1 plan, the current corpus snapshot, and the application
index, and produces the corresponding curated corpus snapshot with its proof
artifacts — the curated manifest, the application report, and the appended
application index record.

```text
serialized DryRunPlan + corpus snapshot + application index
  → path preflight + index single-writer lock   (§6 step 1, §8)
  → strict artifact parsing                      (§6 step 2)
  → snapshot load + tree agreement               (§6 step 3)
  → application-index validation                 (§6 step 4)
  → plan verification (Slice-1 verify_plan)      (§6 step 5)
  → already-applied + application-chain law      (§6 step 6–7)
  → replacement-authority law                    (§6 step 8)
  → staged fresh output tree
      + curated songs manifest
      + application report                       (§6 step 9–10)
  → publish output (one rename)                  (§6 step 11)
  → append application index (the commit point)  (§6 step 12)
```

Every arrow above is an executable law with a failure case in §6–§11; the
refusal taxonomy in §12 is closed; the RED→GREEN matrix in §14 is
preregistered.

### Non-goals (Slice 2 must NOT)

- implement suggestion generation, metadata normalization, or any Slice-3
  behaviour;
- run the controlled pilot or perform production / real-corpus / full-corpus
  labeling (prohibited until the pilot gate, ADR-0033 Decision 10);
- issue new `SongId`s or enforce `next_song_seq` issuance semantics — labels
  applied come entirely from ledger events already carrying their ids; Slice 1
  explicitly deferred issuance enforcement and Apply does not need it;
- change ADR-0031, ADR-0032, or accepted ADR-0033; modify S7, S8, S9, S15, or
  S16; modify Swang or `swang/`;
- add cockpit/CLI integration, ordinary generation behaviour, or teach
  `griff manifest` anything (strategy 2 stays a later, separately accepted
  change — ADR-0033 Decision 8);
- add similarity, embeddings, MIR, audio fingerprinting, or automatic song
  identity;
- turn `song-curation/` into a workspace member, or add a production
  dependency;
- modify the frozen Slice 1 public API or its semantics (§9 states the one
  permitted internal extension, proven behaviour-preserving by the frozen
  Slice-1 test suite staying green);
- perform "helpful reconciliation" of any kind: no normalizing
  partially-labelled untouched sources, no clearing labels, no repairing
  inconsistent history.

## 3. Frozen Slice-1 properties consumed

Slice 2 builds on, and must not create a second interpretation of, the
properties already established and tested by Slice 1 (see
`song-curation/README.md`): exact-`sha256` source identity; ledger validation
before batch selection/projection; one plan embedding exactly one complete
ordered batch; strict (`deny_unknown_fields`) deserialization of the whole
ledger/plan graph; order-sensitive `decisions_digest`; order-aware
`plan_digest` excluding its own field; order-insensitive `corpus_fingerprint`
with the `manifest_songs` absent/present marker; `verify_plan` validating the
embedded batch first, recomputing digests and fingerprints, and re-deriving
assignments and the complete post-plan `generated_songs_map` rather than
trusting them; untouched existing labels surviving into that map; and
replacement authority belonging only to `correct` / `merge` / `split`.

No contradiction between these properties and ADR-0033 was found during the
preparation of this contract.

## 4. Exact Apply inputs and outputs

### 4.1 Inputs

| Input | Form | Authoritative for |
|---|---|---|
| `plan` | one JSON file: a serialized `song-curation.plan.v1` `DryRunPlan` | the decisions being applied (via its embedded batch), the derived assignments and post-plan songs map, the digests, and the plan's corpus binding |
| `corpus` | one directory: the current corpus snapshot (§4.2) | the current label state and the current corpus fingerprint (via its root `manifest.json`) |
| `index` | one JSON file: a `song-curation.applications.v1` application index (§5.1) | what has already been applied, and the chain head |
| `output` | one path that must not exist yet | where the curated snapshot is published |

There is exactly **one authority per fact** and no fact with two independent
sources:

- *Which decisions apply* — the plan's embedded batch, alone. The ledger is
  **not** an Apply input; Slice 1 already bound the batch into the plan.
- *Current label state and fingerprint* — the corpus root `manifest.json`.
  The per-source chunk files are the storage Apply rewrites; they must agree
  with the manifest (§4.2) and are never a second authority.
- *Already-applied and chain position* — the application index, alone. Corpus
  side effects are never used to infer application state (a rejection-only or
  fingerprint-neutral batch has none).
- *Curated-manifest target* — not an input. Its path is fixed by this contract
  (§4.3); v1 accepts no override.

The index file **must exist**. A missing index is a typed refusal
(`MalformedApplicationIndex`), never an implicit empty chain — silently
treating a mistyped path as "nothing applied yet" would erase the
already-applied proof. The initial empty index is created deliberately by the
curator as exactly:

```json
{
  "schema": "song-curation.applications.v1",
  "applications": []
}
```

### 4.2 The corpus snapshot

A snapshot is a directory tree containing:

- a **root `manifest.json`** — a core `CorpusManifest`. Required; its absence
  is `CorpusTreeDisagreement`. This file is what Slice-1 planning fingerprinted
  and is the fingerprint authority for Apply.
- **`*.chunk.json` files** — one core `ChunkMeta` each, enumerated by the
  `migrate/` discipline: recursive walk, sorted relative paths.
- anything else (`*.group.json`, stray files) — corpus content that Apply
  carries but never interprets.
- a reserved **`song-curation/` subdirectory** (§4.3) — curation artifacts,
  **excluded** from corpus-content enumeration. A chunk file or `manifest.json`
  under it is never read as corpus content.

**Tree agreement (fail-closed precondition).** The parsed chunk records of the
root manifest and the parsed on-disk `*.chunk.json` records must be equal as
multisets of `ChunkMeta` values (core `PartialEq`). Any divergence — a chunk in
the manifest with no file, a file absent from the manifest, or a field-level
disagreement — is `CorpusTreeDisagreement`, refused before any other corpus
work. This is what makes "the manifest is the authority, the files are the
storage" safe: they are proven to say the same thing before either is used.

**Ordinary manifest must not carry `songs`.** If the root `manifest.json` has
`songs: Some(..)`, Apply refuses (`OrdinaryManifestCarriesSongs`). Rationale:
the v1 chain keeps the songs cross-check exclusively at the curated distinct
path (ADR-0033 Decision 8). A root-level map would either be preserved stale
(disagreeing with the new labels — an integrity fault) or rewritten (exactly
the ordinary-path write the ADR forbids). Refusing is the only fail-closed
option, and it keeps the `manifest_songs` fingerprint marker uniformly
`absent` at the root across every v1 chain snapshot.

Note on parsing strictness: the strict `deny_unknown_fields` boundary covers
the **curation artifact graph** (plan, embedded ledger types, application
index, application report). The core corpus schema (`ChunkMeta`,
`CorpusManifest`) deliberately does not deny unknown fields and Slice 2 does
not change core; the preservation guard in §10 is what prevents tolerant
parsing from ever laundering a corpus file.

### 4.3 Outputs

All outputs land under the output root; the reserved area keeps proof
artifacts out of corpus content:

| Output | Path (fixed, v1) |
|---|---|
| curated corpus snapshot | `<output>/…` — same relative layout as the input snapshot |
| curated songs manifest | `<output>/song-curation/manifest.json` |
| application report | `<output>/song-curation/apply-report.json` |
| updated application index | in place at the `index` input path (§8) |

The curated-manifest path is fixed and distinct by construction. The
implementation must still hard-refuse (`CuratedManifestPathNotDistinct`) any
resolved curated-manifest target equal to `<tree-root>/manifest.json` of the
input or output tree — defense in depth so the ADR-0033 Decision 8 guard
survives any future path-rule change, and ordinary `griff manifest` (which
writes only `<dir>/manifest.json`, `cli/src/main.rs::cmd_manifest`) can never
collide with it.

## 5. Artifact schemas

### 5.1 Application index v1

Schema id: `song-curation.applications.v1`. One JSON document:

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

Laws:

- **Strict deserialization.** Every index type carries
  `deny_unknown_fields`; a foreign field at any depth is
  `MalformedApplicationIndex`. All four record fields are required.
- **Schema identity.** `schema` must equal
  `song-curation.applications.v1`; anything else is
  `UnsupportedApplicationIndexSchema`.
- **Ordered records.** Array order is the application order and is
  authoritative; there is no per-record ordinal.
- **Record identity.** A record is identified by its `batch_id`; `batch_id`s
  are unique within the index (`DuplicateAppliedBatchId` — an index-internal
  corruption, distinct from `DecisionBatchAlreadyApplied`, which is about the
  incoming plan).
- **Internal chain validity.** For every adjacent pair,
  `applications[i].input_corpus_fingerprint ==
  applications[i-1].output_corpus_fingerprint` must hold
  (`ApplicationIndexChainInvalid`, carrying the position and both values). A
  malformed history refuses before the incoming batch is even considered.
- **Initial-chain semantics.** An empty `applications` array is the valid
  initial state; the first appended record has no predecessor constraint
  beyond §7.1.
- **Non-initial semantics.** The last record is the chain head; §7.2 binds the
  incoming batch to it.
- **No previous-report field.** A record does not repeat its predecessor's
  `report_digest`; adjacency in the ordered array *is* the predecessor
  relation, and the incoming batch's `previous_application_report_digest` is
  checked against the head record's `report_digest` (§7.2). Duplicating the
  link inside each record would create a second source for the same fact.
- **No filesystem paths.** The chain is content-addressed (fingerprints and
  digests); paths are machine-local and are not identity. The index alone —
  not the report file's continued existence — is the proof that a batch was
  applied.
- **Deterministic serialization.** Written as pretty-printed JSON in struct
  field order (the corpus rendering convention); identical records produce
  identical bytes.

The index is an explicit Apply input and the **only** authority for
"already applied". A rejection-only or fingerprint-neutral batch leaves no
corpus side effect, but its index record still exists, so it is still provably
applied (§7.3).

### 5.2 Application report v1

Schema id: `song-curation.apply-report.v1`. One JSON document, written at
`<output>/song-curation/apply-report.json`:

```json
{
  "schema": "song-curation.apply-report.v1",
  "batch_id": "batch-000004",
  "applied_event_ids": ["ev-000017", "…in batch order…"],
  "input_corpus_fingerprint": "<hex>",
  "output_corpus_fingerprint": "<hex>",
  "decisions_digest": "<hex>",
  "plan_digest": "<hex>",
  "previous_application_report_digest": null,
  "curated_manifest_path": "song-curation/manifest.json",
  "curated_manifest_digest": "<hex>",
  "assignments_applied": 0,
  "assignments_unchanged": 0,
  "sources_reviewed_unassigned": 0,
  "sources_untouched": 0,
  "coverage": {
    "unique_sources": 0,
    "labelled": 0,
    "unlabelled": 0,
    "songs": 0
  },
  "holdout_ready": false,
  "holdout_refusals": [
    { "kind": "uncurated_source", "sha256": "…", "song_id": null }
  ],
  "report_digest": "<hex>"
}
```

Field laws — every field has a proof purpose; none duplicates another:

- `batch_id`, `applied_event_ids` — which batch, which events, in the batch's
  authoritative order.
- `input_corpus_fingerprint` / `output_corpus_fingerprint` — the chain link
  this application consumed and produced. The output fingerprint is computed
  over the **published output root `manifest.json`** by the Slice-1
  `corpus_fingerprint`, re-read from staged bytes before publication (§8),
  never from an in-memory value alone.
- `decisions_digest`, `plan_digest` — bind the report to the exact plan
  applied.
- `previous_application_report_digest` — the chain predecessor (`null` for an
  initial application). Adopted beyond the historical proposal's report schema
  so the report self-describes its chain position; rationale in §13.
- `curated_manifest_path` — the fixed relative path (§4.3); relative so the
  report stays machine-independent and byte-deterministic.
- `curated_manifest_digest` — SHA-256 of the published curated-manifest file's
  exact bytes, so `sha256sum` verifies it with no canonicalization step.
- `assignments_applied` — assignments whose label was written (source was
  unlabelled or carried an authorized-superseded label);
  `assignments_unchanged` — assignments already correct on disk (ADR-0033
  Decision 6: treated as unchanged); `sources_reviewed_unassigned` — sources
  whose final batch state is a review without assignment (the
  `reject_suggestion` outcome: referenced and validated, no label produced —
  ADR-0033 keeps this distinct from "never reviewed"); `sources_untouched` —
  sources the batch did not reference at all. The four counts partition the
  snapshot's sources exactly: their sum equals `coverage.unique_sources`
  (a rejection-only batch has zero applied/unchanged and non-zero
  reviewed-unassigned, §14 C6).
- `coverage` — post-apply totals over the output snapshot.
- `holdout_ready` / `holdout_refusals` — §11. `holdout_refusals` records the
  core preflight refusals observed on the curated output view (each as
  `{ kind, sha256|null, song_id|null, chunk_id|null }`, `kind` the
  snake_case core `SongHoldoutRefusal` variant name), sorted by the tuple
  `(kind, sha256, song_id, chunk_id)` with `null` ordering before any value.
- `report_digest` — over the whole report per §5.4.
- **No wall-clock timestamp.** The report carries no "applied at" time;
  event `occurred_at` values live in the ledger. This keeps repeated execution
  from identical inputs byte-identical (§14 F5).

### 5.3 Curated songs manifest

The curated manifest at `<output>/song-curation/manifest.json` is a complete
core `CorpusManifest`, equal to the published output root `manifest.json` in
`schema_version`, `chunks`, and `groups`, with exactly one difference:
`songs: Some(map)` where `map` is the plan's verified `generated_songs_map` —
`SongId → sorted, unique [sha256]`, exactly the deterministic projection
Slice 1 already derives and `verify_plan` already re-derives. Per-source
`SourceRef.song_id` remains the source of truth (ADR-0031); this artifact is
the cross-check.

**Manifest agreement holds by construction** (the map is derived from the same
labels the output carries) and is additionally proven at apply time: the
staged holdout preflight (§11) runs over this curated view, and its
bidirectional agreement checks (`ManifestSourceMissing` /
`ManifestLabelMissing`) must find nothing — if they do, that is
`OutputPreflightInconsistent`, an aborted apply, because it can only mean an
implementation bug.

**Slice 2 owns curated-manifest generation.** Repository evidence: ADR-0033
Decision 6 makes report + index publication part of Apply and the report
schema must name the curated manifest's path and digest; Decision 8 assigns
the distinct-path guard to "the tool" with no later slice; and the accepted
Slice-1 README calls `generated_songs_map` "the map that becomes the Slice-2
manifest projection". A valid applied snapshot without its cross-check
artifact would be un-provable, so the artifact belongs to the same
transactional unit.

### 5.4 Digest contract for `report_digest`

`report_digest` extends the Slice-1 shared canonical encoding (compact UTF-8
JSON, object keys sorted, array order preserved) exactly as the historical
proposal's contract 4, adopted here verbatim with one extension:

- canonicalize the complete report with the `report_digest` field omitted;
- `applied_event_ids` in batch order (order-preserving);
- `holdout_refusals` sorted by `(kind, sha256, song_id, chunk_id)` — the
  proposal's `(kind, source_sha256)` tuple extended, because core preflight
  refusals can carry a `song_id` or `chunk_id` and the sort must be total;
- every other array in the report is already deterministically ordered by its
  own field law.

`report_digest` covers every report field except itself and excludes nothing
else. The digest the next batch's `previous_application_report_digest` cites
is this one.

## 6. Verification order — the fail-closed sequence

Apply executes exactly this order. **No step may create, modify, or delete
anything under the corpus trees, the output path, or the index file before
step 9** — the single deliberate exception is the transient index lockfile
(step 1, §8.1): coordination state, never output, part of no published
artifact. A malformed input can therefore never leave a partial output tree.

1. **Path preflight, then the lock.** The path checks are pure — they read
   nothing but path metadata, under the `migrate/` canonicalization
   discipline (resolve symlinks via `canonicalize`; resolve the
   not-yet-existing output and staging paths as canonical parent + final
   component); the lock acquisition closing this step is the one exception
   named in the preamble:
   - output path already exists, or the staging path (§8) already exists →
     `OutputAlreadyExists { path }`;
   - resolved output (or staging) equals, contains, or is contained by the
     resolved input corpus root → `OutputWouldModifyInput`;
   - resolved index path lies inside the input corpus root, the output root,
     or the staging root → `ApplicationIndexInsideTree { path }`;
   - resolved curated-manifest target equals a tree root's `manifest.json` →
     `CuratedManifestPathNotDistinct { path }` (structurally unreachable in
     v1; kept as a hard guard, §4.3);
   - every staging/temporary name is derived only from the output and index
     paths (§8.1), never from plan content — so this step needs nothing
     parsed, and a `batch_id` (an unrestricted Slice-1 `String`) can never
     influence a filesystem path;
   - finally, acquire the **index single-writer lock** (§8.1): atomically
     create the lockfile next to the **canonical resolved index file**; a
     pre-existing lockfile → `ApplicationIndexLocked { path }`; a create or
     release failure that is *not* pre-existence (permissions, read-only
     filesystem, …) → `ApplyIoError { path, op, detail }`. The lock is held
     through step 12 and released on every exit (§8.2).
2. **Strict artifact parsing.** Parse the plan file with the Slice-1 strict
   types (`MalformedPlanArtifact { detail }` on any failure, foreign fields
   included) and the index file with the strict §5.1 types
   (`MalformedApplicationIndex { detail }`). Parsing precedes every
   filesystem mutation and every digest/projection computation.
3. **Snapshot load and agreement.** Read the root `manifest.json` and every
   corpus `*.chunk.json` (reserved area excluded); enforce tree agreement and
   the no-root-`songs` law (§4.2): `CorpusTreeDisagreement` /
   `OrdinaryManifestCarriesSongs`.
4. **Application-index validation** (§5.1): schema, uniqueness, internal
   chain: `UnsupportedApplicationIndexSchema` / `DuplicateAppliedBatchId` /
   `ApplicationIndexChainInvalid`.
5. **Plan verification — the Slice-1 `verify_plan`, reused verbatim** against
   the loaded root manifest. This is a literal call, not a reimplementation
   (§9). It already enforces, in Slice 1's internal order: embedded-batch
   structural validity first (short-circuiting), then digest recomputation
   (`PlanDigestMismatch` / `DecisionDigestMismatch`), the two separate
   corpus-fingerprint bindings against the **current** corpus
   (`PlanCorpusFingerprintMismatch` / `DecisionBatchFingerprintMismatch` —
   the drift refusal), schema/policy identity, and full re-derivation of
   assignments and songs map (`DecisionProjectionMismatch`, plus any
   inventory/replay refusal). Corpus-fingerprint checking therefore happens
   here, after index validation and before any chain comparison, so chain
   equations only ever run against a plan already proven to bind to the
   current corpus.
6. **Already-applied check** (§7.3): the plan batch's `batch_id` present in
   the index → `DecisionBatchAlreadyApplied { batch_id }`. Checked **before**
   the chain equations so a re-used batch is reported as re-used, not as a
   chain mismatch (its chain fields would typically also disagree; the
   already-applied fact is the more specific refusal — ADR-0033 Decision 6
   names it explicitly).
7. **Application-chain law** (§7.1–7.2): any broken relation →
   `ApplicationChainMismatch`.
8. **Replacement authority** (§7.4): per assignment, against the on-disk
   label and the acting event's authorized supersession set →
   `ExistingLabelReplacementNotAuthorized`.
9. **Stage the snapshot** (§8): create the staging directory; write the
   corpus files under the preservation law (§10) — rewritten touched files,
   raw-copied untouched files — and the curated manifest. The report is
   **not** written yet: its fields do not all exist before step 10. The
   preservation guard (`NonCanonicalCorpusFile`) fires here, before any
   modified byte is staged for the affected file. Any I/O failure →
   `ApplyIoError { path, op, detail }` and best-effort staging cleanup.
10. **Staged self-check, then the report.** Re-read the staged root
    `manifest.json` and the staged curated manifest from bytes; **re-run the
    §4.2 tree-agreement law over the staged tree** (staged root manifest ↔
    staged `*.chunk.json` files, the same multiset check as step 3 — not a
    second preflight), so a buggy write that updated the manifests but
    missed an affected chunk file, or the reverse, can never publish;
    recompute the output fingerprint and the curated-manifest digest; run
    the core `song_holdout_preflight` — its single execution (§11) — over
    the curated view. A staged-agreement violation, a recomputation
    divergence, or a non-`UncuratedSource` preflight refusal →
    `OutputPreflightInconsistent` (abort, cleanup staging). Only then build
    the report (§5.2) from these verified values, compute `report_digest`,
    and write the report into the staging tree.
11. **Publish the snapshot**: one `rename(staging → output)`.
12. **Commit**: append the index record via write-temp + `rename` over the
    index path (§8), then release the index lock. Only after this rename is
    the batch applied.

## 7. Chain, already-applied, and authority laws

Notation: `P` = the plan, `B = P.decision_batch`, `I` = the validated index,
`L` = the last record of `I.applications` (when non-empty), `F_in` = the
Slice-1 `corpus_fingerprint` of the loaded root manifest. Step 5 has already
proven `P.input_corpus_fingerprint == F_in` and
`B.input_corpus_fingerprint == F_in`.

### 7.1 Initial batch (`I.applications` is empty)

Exactly one relation:

```text
B.previous_application_report_digest == null
```

A non-`null` value with an empty index is `ApplicationChainMismatch`. The
corpus binding needs no additional initial rule: `B.input_corpus_fingerprint
== F_in` is already enforced by step 5.

### 7.2 Later batch (`I.applications` is non-empty, head `L`)

All of the following equalities, each independently checked; the first broken
one is the refusal's evidence (relation name, expected value, actual value):

```text
(1) B.previous_application_report_digest == L.report_digest      # non-null
(2) B.input_corpus_fingerprint          == L.output_corpus_fingerprint
(3) L.output_corpus_fingerprint         == F_in
```

A `null` `previous_application_report_digest` against a non-empty index breaks
relation (1). Relation (3) is entailed by (2) plus step 5 but is stated and
checked, so a violation is attributed to the chain (the corpus is not the
snapshot the registry says the chain head produced) rather than surfacing only
as a step-5 drift refusal. Any broken relation is
`ApplicationChainMismatch { relation, expected, actual }`. There is no other
"valid chain" condition.

### 7.3 Already-applied semantics

`DecisionBatchAlreadyApplied { batch_id }` fires **iff** the plan batch's
`batch_id` appears in `I.applications` — a registry fact, never an inference
from corpus state. ADR-0033 Decision 6: re-use of a `batch_id` is *not* an
idempotent re-application. Four distinct situations, only the first refuses:

- **Same `batch_id` in this index** → refuses, even when the batch was
  rejection-only or otherwise fingerprint-neutral and the corpus shows no side
  effect (this is precisely why the index, not the corpus, is the authority).
- **An already-correct individual label** inside a fresh batch → not a
  refusal; the assignment is applied as unchanged and counted in
  `assignments_unchanged` (§5.2).
- **A logically equivalent but new batch** (same events, fresh `batch_id`,
  correctly chained) → a new application: it passes the index check, must
  still satisfy §7.2, and produces its own report and record.
- **An initial batch applied to another copy of the same corpus with its own
  (empty) index** → applies. An index defines one application lineage;
  independent lineages over copies are intentionally possible, and each
  lineage's registry proves its own history.

### 7.4 Existing-label replacement authority

Definitions, per assignment `A` in the verified plan:

- `on_disk(A)` — the single existing label of source `A.source_sha256` in the
  loaded snapshot (`None` when uncurated). Slice-1 inventory has already
  refused conflicting labels, so this is well-defined.
- `acting_event(A)` — the event whose replay effect last set this source's
  batch state (latest-wins order; §9 explains how it is derived from the one
  shared replay).
- `authorized(E)` — the supersession set of event `E`'s action:
  - `correct` → its `supersedes_song_ids`;
  - `merge` → its `from_song_ids`;
  - `split` → `{ from_song_id }`;
  - `accept_suggestion` / `manual_define` → the empty set;
  - `reject_suggestion` → assigns nothing, so no authority question arises.

**Supersession-evidence consistency (checked first).** The frozen Slice-1
`Action` schema also carries a redundant `supersedes_song_ids` field on
`accept_suggestion`, `merge`, and `split`. Before any per-assignment
authority decision, Apply proves for **every** event of the embedded batch
that this evidence cannot contradict the authority law:

- `accept_suggestion`: `supersedes_song_ids` must be empty;
- `merge`: `supersedes_song_ids`, sorted and deduplicated, must equal
  `from_song_ids`, sorted and deduplicated;
- `split`: `supersedes_song_ids` must equal exactly `[from_song_id]`;
- `correct`: its `supersedes_song_ids` *is* the authority, so there is no
  redundant claim to check; `manual_define` and `reject_suggestion` carry no
  supersession field.

Any violation is
`SupersessionEvidenceContradiction { event_id, detail }`: a digest-bound
event must never make two conflicting claims about replacement rights with
Apply silently preferring one of them. This check is deliberately **not** in
the frozen `verify_plan` (which stays untouched); it is part of Slice 2's
authority law, and `authorized(E)` is evaluated only over events that passed
it.

The law, exhaustively by case:

| Case | Condition | Outcome |
|---|---|---|
| assign unlabelled | `on_disk = None` | write; counts as applied |
| already correct | `on_disk = Some(s)`, `s == A.song_id` | no replacement occurs; write-through unchanged; counts as unchanged |
| authorized correct / merge / split | `on_disk = Some(s)`, `s != A.song_id`, `s ∈ authorized(acting_event(A))` | write; counts as applied |
| unauthorized replacement | `on_disk = Some(s)`, `s != A.song_id`, `s ∉ authorized(acting_event(A))` | **`ExistingLabelReplacementNotAuthorized { source_sha256, on_disk_song_id, new_song_id, event_id }`** |

Notes with proof value:

- The matrix case "on-disk label differs from `expected_existing_song_id`"
  needs no separate refusal: the corpus fingerprint covers every
  `(chunk_id, sha256, song_id)` triple, so a label state differing from the
  one the plan was derived against cannot pass step 5
  (`PlanCorpusFingerprintMismatch`), and a forged `expected_existing_song_id`
  inside the plan cannot survive `verify_plan`'s re-derivation
  (`DecisionProjectionMismatch`). §14 L7 exercises exactly this and expects
  the drift refusal.
- Authority is judged against `on_disk`, not against
  `expected_existing_song_id`; the two are provably equal after step 5, and
  the on-disk value is the one a replacement actually overwrites.
- Apply never clears a label: no action produces a `None` effect over an
  existing label (`reject_suggestion` reviews without assigning; the batch
  state `None` means "no assignment", and unassigned sources keep their
  on-disk label — the Slice-1 songs-map law already fixed this).

## 8. Filesystem and transaction semantics

### 8.1 Staging protocol

- Staging root: `<output_parent>/.<output_name>.apply-staging` — a sibling
  of the final output path, so the publication rename never crosses a
  filesystem. Its pre-existence refuses in step 1 (`OutputAlreadyExists`,
  carrying the staging path). **No filesystem name is ever derived from plan
  content**: `batch_id` is an unrestricted Slice-1 `String` (it may contain
  `/`, `..`, or anything else a curator can type), so it appears only inside
  JSON artifacts, never in a path. The fixed staging name needs nothing
  parsed — which is what lets step 1 run before step 2 — and needs no
  uniqueness suffix, because pre-existence refuses and the index lock
  serializes appliers.
- The **complete** output — corpus files, curated manifest, report — is
  written under staging (steps 9–10; the report last, after the single
  preflight, §6). Nothing is ever written at the final output path directly,
  and the input tree and the index file are untouched throughout steps 1–11
  (the lockfile and index temp file are siblings of the index, never the
  index itself).
- **Single-writer lock.** From before the index is first read (step 1) until
  after the commit rename (step 12), Apply holds an exclusive lock: atomic
  creation (`create_new`) of an empty lockfile,
  `<canonical_index_dir>/.<canonical_index_name>.lock`. **The canonical
  resolved index file is the one and only index identity**: the supplied
  index path is `canonicalize`d at step 1 (symlinks fully resolved; the file
  must already exist, §4.1), and that resolved path alone is used for the
  lock name, the index read, the temp file, the fsync, the commit rename,
  and the release — so two aliases of one index always contend on one lock,
  and the commit rename replaces the real file, never a symlink alias. A
  pre-existing lockfile refuses `ApplicationIndexLocked` — fail-closed, no
  waiting, no lock-breaking; any other lock create/release failure is
  `ApplyIoError`. Without it, two concurrent
  appliers could read one chain head, both pass every check, publish two
  output trees, and each serialize the index from the same stale version:
  the second commit rename would silently drop the first record after its
  applier had already reported success — violating the very definition of
  success (§8.2). The lock closes the read-to-commit window; a second
  applier run *after* the first completes is still refused by the chain law
  itself — the same batch by `DecisionBatchAlreadyApplied` (§7.3), a batch
  planned against the stale head by a broken §7.2 relation (for a
  fingerprint-neutral head that is the previous-report-digest relation (1),
  not any fingerprint change).
  The lock is released (deleted) on every exit, success or refusal,
  best-effort; a crash can leave it, and the stale lock then refuses every
  future apply until an operator verifies no apply is running and removes it
  (§8.2). A failed best-effort release never changes the run's primary
  outcome — a committed apply stays applied, a refusal stays that refusal —
  but it must be surfaced explicitly alongside that outcome, naming the
  leftover lockfile and pointing at the §8.2 recovery. The lockfile is empty
  and transient — not a deliverable artifact, excluded from the determinism
  law (§10.5).
- Publication (step 11) is a single `rename(staging, output)`: the snapshot
  and its in-tree proof artifacts become visible atomically (POSIX rename
  semantics on one filesystem), or not at all.
- Commit (step 12): serialize the updated index (§5.1), write it to
  `<canonical_index_dir>/.<canonical_index_name>.tmp`, flush and sync the
  file, then `rename`
  over the index path. The temp file lives in the index's own directory, so
  this rename is also same-filesystem and atomic. A leftover temp file found
  under a freshly acquired lock is crash debris of a previous run — the held
  lock proves no live writer exists — and is deleted before writing.

### 8.2 The failure model — precisely what "transactional" means here

This contract claims **no more atomicity than plain-file operations provide**.
There is no multi-file atomic commit over a POSIX filesystem; the honest
mechanism is a **single committed point** with enumerable crash states:

**The batch is applied iff its record is in the application index.** That is
the entire definition of success. The report and output tree are evidence the
index record cites (by digest and fingerprint); their existence alone proves
nothing.

Enumerated states after a failure or crash, and the mandated recovery:

| State | Observable as | Meaning | Recovery |
|---|---|---|---|
| failure in steps 1–8 | nothing written anywhere (the lock, if already acquired, is released; a failed release leaves it — see the lock row) | not applied | none needed |
| failure in steps 9–10 | staging dir present (cleanup is best-effort; a failed cleanup leaves it), output absent, index unchanged | not applied | delete the staging dir; a retry that finds it refuses `OutputAlreadyExists` rather than silently reusing it |
| crash between steps 11 and 12 | output tree present **with** report, index has **no** matching record | **not applied** — the registry is the authority | delete the orphaned output tree; a retry refuses `OutputAlreadyExists`, forcing the operator to confront the orphan instead of stacking a second tree |
| step 12 completed | index record present | applied | none |
| failure inside step 12 (temp write, fsync, or commit rename) | output tree already published **with** report (step 11 preceded step 12), old index unchanged, temp index file possibly present | not applied | delete the temp file **and** the orphaned published output tree — this is the previous row's state plus temp debris; a retry refuses `OutputAlreadyExists` until the orphan is removed |
| crash at any point after lock acquisition | lockfile `.<canonical_index_name>.lock` present, possibly alongside one of the states above | no apply may start against this index | verify no apply is running, then delete the lockfile (and any leftover `.<canonical_index_name>.tmp`), then perform the recovery of whichever state above also holds |

Consequences stated plainly:

- "Report publication and index append are transactional" (ADR-0033
  Decision 6) is implemented as: the report can never be *committed* without
  its index record, because commitment **is** the index record. A crash can
  leave report bytes on disk without a record; those bytes are provably
  not-applied staging debris, and the registry never lies. The reverse — an
  index record without its report having been written — cannot occur, because
  step 12 strictly follows step 11.
- Success is reported to the operator only after step 12 returns. An error
  reported from steps 11–12 must state exactly which of the enumerated states
  the filesystem is in.
- **Durability caveat:** the index temp file is flushed and synced before the
  commit rename; directory-entry durability across power loss is
  filesystem-dependent and is **not** claimed. The guarantees above concern
  visibility ordering and atomicity on a running system.
- No "ACID" vocabulary applies beyond the above and none is claimed.

### 8.3 Path safety

All input roots and the index location are canonicalized before comparison
(the `migrate/` discipline), so symlink aliases cannot smuggle the output into
an input tree, the index into the output tree, or vice versa (§6 step 1). The
output and staging paths, which do not exist yet, are resolved as canonical
parent + final component, exactly as `preflight_output` does.

## 9. One semantic authority — reuse, not reimplementation

- **Plan verification is a literal reuse of Slice-1 `verify_plan`.** Apply
  calls it (step 5) at the same serialized-artifact boundary Slice 1 tested;
  no digest, fingerprint, replay, or projection check is reimplemented.
  There is exactly one interpretation of a batch in the codebase.
- **Replay attribution is the one permitted internal extension.** The §7.4
  authority law needs, per assigned source, the *acting event* — which the
  Slice-1 public API does not expose. The implementation extends the crate's
  single internal replay primitive to also record, per source, the `event_id`
  of the latest effect, and derives both `verify_plan`'s projection and
  Slice 2's attribution from that one primitive. Constraints, enforced at
  acceptance of the implementation: the frozen Slice-1 public API, semantics,
  and complete test suite remain untouched and green; no second replay is
  written; the extension changes no observable Slice-1 behaviour.
- The `apply` entry point is new Slice-2 public API in the same isolated
  `song-curation` crate (a new module; the crate stays a non-workspace-member
  isolated tool, ADR-0010 posture).

## 10. Preservation law — the audited "byte-for-byte" claim

**Audit of the historical claim.** The proposal promised "preserving every
unrelated field byte-for-byte (only `SourceRef.song_id` changes)". Repository
evidence shows this is not implementable as literally stated for rewritten
files, and was never what the precedent did:

- `migrate/` **re-serializes** every file it emits via
  `serde_json::to_string_pretty` (`migrate/src/main.rs::run`) — it preserves
  parsed semantics, not input bytes;
- core corpus types are **not** `deny_unknown_fields` (`ChunkMeta`,
  `CorpusManifest`, `SourceRef` in `core/src/corpus.rs`), so a plain
  parse→modify→serialize of a file carrying fields unknown to the current
  schema would *silently drop them* — laundering, not preservation;
- JSON byte formatting (whitespace, numeric rendering such as `120` vs
  `120.0`) is not part of parsed semantics, so byte identity through a parse
  cannot be promised.

**The adopted law** (explicitly bounded; neither silently weakened nor blindly
repeated):

1. **Untouched files: byte identity.** Every file in the input tree that is
   not *touched* (defined below) is published into the output by **raw byte
   copy**. Byte-for-byte preservation holds absolutely here — including
   `*.group.json` and any file the tool does not interpret — because the
   bytes are never decoded on the write path.
2. **Touched files: semantic identity outside the label, canonical
   rendering.** A chunk file is *touched* iff it contains a chunk whose
   `source.sha256` is in the plan's assignments; the root `manifest.json` is
   touched iff the assignment list is non-empty. A touched file is parsed
   with the core types, only `source.song_id` of assigned chunks is changed,
   and it is re-serialized with `serde_json::to_string_pretty` — the corpus
   rendering convention shared by `cli` and `migrate`. Every JSON member
   other than the assigned `source.song_id` members is preserved as a parsed
   value exactly; byte formatting of touched files is canonical, not
   input-verbatim.
3. **Fail-closed laundering guard** — two checks on every touched file, each
   catching what the other cannot, both refusing
   `NonCanonicalCorpusFile { path, detail }` before that file's modified
   bytes are staged:
   - **Duplicate-key rejection.** The file's raw text is parsed by a
     duplicate-rejecting pass: a JSON deserialization whose map visitor
     refuses a repeated object key at **any** depth (native code over
     `serde`, no new dependency). This must be a distinct pass —
     `serde_json::Value` is already a map that silently keeps the last
     duplicate, so no comparison of `Value`s can ever prove duplicates were
     absent.
   - **Round-trip equality.** `to_value` of the *unmodified* parsed record
     must equal the raw `serde_json::Value` parse of the file — catching an
     unknown member the tolerant schema would drop, and any value the parse
     re-renders differently (e.g. a numeric formatting change).
   Tolerant core parsing therefore can never silently launder a rewritten
   file; a file that would lose anything refuses instead.
4. **Every-chunk-together.** All chunks sharing an assigned `sha256` are
   updated in the same apply — in every chunk file that carries them and in
   the root manifest — which tree agreement (§4.2) plus assignment
   `affected_chunk_ids` make checkable.
5. **Determinism.** Byte-identical inputs (corpus bytes, plan bytes, index
   bytes) produce byte-identical outputs — tree, curated manifest, report,
   and updated index (no wall-clock, no randomness, sorted traversal
   everywhere).

A fingerprint-neutral apply (no assignments) touches nothing: every corpus
file, including the root manifest, is raw-copied, and
`output_corpus_fingerprint == input_corpus_fingerprint` follows by
construction (§14 C6 proves the index still records it).

## 11. Holdout readiness

Slice 2 executes the existing core `song_holdout_preflight` — never a
near-copy — exactly once per apply, at step 10, over the **staged curated
view**: the staged output chunks with `songs: Some(generated_songs_map)`
(i.e., the curated manifest, §5.3). Laws:

- `holdout_ready: true` in the report **iff** the preflight returns `Ok(())`.
- **Partial curation is a successful snapshot.** `UncuratedSource` refusals —
  sources still `song_id: None` — do not abort the apply; they are recorded
  (sorted, §5.2) in `holdout_refusals` and force `holdout_ready: false`. A
  valid partial snapshot and a complete holdout-ready corpus are distinct
  states and are never conflated (ADR-0033 Decision 9); a partial curated
  corpus can never masquerade as holdout-ready because the flag is computed
  by the real preflight, not asserted.
- **Genuine inconsistency is not incompleteness.** Any preflight refusal
  *other than* `UncuratedSource` on the staged output —
  `UnidentifiedSource`, `InconsistentSource`, `ManifestSourceMissing`,
  `ManifestLabelMissing` — is `OutputPreflightInconsistent` and aborts before
  publication. After steps 3, 5, and 8, such a state is unreachable except
  through an implementation bug, and an apply must not publish evidence of
  its own bug as a curated snapshot.
- The result is stored in the report (`holdout_ready`, `holdout_refusals`);
  Apply has no "require holdout ready" mode in v1 (§13 rejects the
  proposal's flag for this slice).

## 12. Refusal taxonomy — closed for Slice 2

Slice 2's refusal surface is the Slice-1 set (reused verbatim through step 5)
plus the new typed refusals below. This table is **closed**: an implementation
may not add, merge, or rename members without re-acceptance. "Output?" states
whether any filesystem output may exist when the refusal is returned
(`staging†` = a staging directory may remain only if best-effort cleanup
itself failed; never the final output path).

Slice-1 refusals **reachable through step 5's `verify_plan`**, reused with
unchanged meaning — eleven members: `UnidentifiedSource`,
`ConflictingExistingSongIds`, `UnknownDecisionSource`,
`SourceAssignedToMultipleSongs`, `InvalidDecisionBatchOrder`,
`DuplicateDecisionEventId`, `PlanCorpusFingerprintMismatch`,
`DecisionBatchFingerprintMismatch`, `DecisionDigestMismatch`,
`PlanDigestMismatch`, `DecisionProjectionMismatch`. All are pre-staging: no
output exists.

The remaining three members of the shared crate error type —
`UnsupportedDecisionsLedgerSchema`, `DuplicateDecisionBatchId`,
`BatchNotInLedger` — are ledger-side: they arise only in Slice 1's
`validate_ledger` / `build_plan`, and Apply consumes a plan, never a ledger.
They are therefore **intentionally unreachable** in Slice 2, are not part of
the Apply refusal surface, and are excluded from the §14 coverage claim.

New Slice-2 refusals:

| Refusal | Condition | Evidence carried | Ordering | Output? |
|---|---|---|---|---|
| `OutputAlreadyExists` | final output or staging path exists (also: retry after an orphaned publication) | the colliding path | step 1 | no (pre-existing paths are not this run's output) |
| `OutputWouldModifyInput` | resolved output/staging equals, contains, or is contained by the input root | both resolved paths | step 1 | no |
| `ApplicationIndexInsideTree` | resolved index path inside input, output, or staging root | the resolved index path | step 1 | no |
| `CuratedManifestPathNotDistinct` | resolved curated target is a tree root's `manifest.json` (defense in depth; unreachable under the v1 fixed path) | the resolved path | step 1 | no |
| `ApplicationIndexLocked` | the index lockfile already exists at acquisition (a concurrent apply, or a stale lock from a crash) | the lockfile path | step 1, before the index is read | no |
| `MalformedPlanArtifact` | plan file unreadable, not valid JSON, or refused by the strict Slice-1 types (foreign field at any depth) | detail (path + parse error) | step 2 | no |
| `MalformedApplicationIndex` | index file missing, unreadable, not valid JSON, or refused by the strict §5.1 types | detail (path + parse error) | step 2 | no |
| `CorpusTreeDisagreement` | root `manifest.json` missing, or manifest chunks ≠ on-disk chunk records (§4.2) | detail: the first divergence (chunk id / path / field) | step 3 | no |
| `OrdinaryManifestCarriesSongs` | root `manifest.json` has `songs: Some(..)` | — | step 3 | no |
| `UnsupportedApplicationIndexSchema` | index `schema` ≠ `song-curation.applications.v1` | the rejected schema string | step 4 | no |
| `DuplicateAppliedBatchId` | one `batch_id` twice **within** the index (internal corruption) | the duplicated `batch_id` | step 4 | no |
| `ApplicationIndexChainInvalid` | adjacent index records break `input[i] == output[i-1]` | position + both fingerprints | step 4 | no |
| `DecisionBatchAlreadyApplied` | plan batch's `batch_id` present in the index | the `batch_id` | step 6 — after plan verification, **before** chain equations (§6) | no |
| `ApplicationChainMismatch` | any §7.1/§7.2 relation broken | relation name, expected, actual | step 7 | no |
| `SupersessionEvidenceContradiction` | an event's redundant `supersedes_song_ids` contradicts its action's authority set (§7.4 consistency law) | `event_id` + detail | step 8, before any per-assignment authority decision | no |
| `ExistingLabelReplacementNotAuthorized` | unauthorized-replacement case of §7.4 | `source_sha256`, on-disk label, new label, acting `event_id` | step 8, after the consistency law | no |
| `NonCanonicalCorpusFile` | a touched file fails either §10.3 guard (duplicate object key at any depth, or round-trip divergence) | path + divergence detail | step 9, before that file's modified bytes are staged | staging† |
| `ApplyIoError` | an I/O operation fails during lock acquisition/release (any cause but pre-existence) or during staging/publication/commit | path, operation, OS error | step 1 (lock I/O) and steps 9–12 | no for lock I/O; staging† during staging; after step 11, the §8.2 state table governs |
| `OutputPreflightInconsistent` | staged tree-agreement violation (§4.2 law re-run over the staged tree), recomputation divergence, or a non-`UncuratedSource` preflight refusal on the curated view (§6 step 10, §11) | the divergent value, the disagreeing chunk, or the preflight refusals | step 10 | staging† |

Historical names reconciled: `PlanDigestMismatch`, `DecisionDigestMismatch`,
`PlanCorpusFingerprintMismatch`, `DecisionProjectionMismatch` are Slice-1
refusals reused unchanged. `DecisionBatchAlreadyApplied`,
`ApplicationChainMismatch`, `ExistingLabelReplacementNotAuthorized`,
`OutputAlreadyExists`, `OutputWouldModifyInput` are defined above with the
proposal's intended semantics. The proposal's `ManifestDisagreement` is not a
Slice-2 refusal: agreement holds by construction (§5.3) and its violation is
the implementation-bug abort `OutputPreflightInconsistent`. The proposal's
`IncompleteCoverage` is not a Slice-2 refusal: incompleteness is the
`holdout_ready: false` status (§11); a "require ready" gate belongs to the
pilot slice if anywhere (§13). There is deliberately no `ApplyFailed(String)`,
and equally no scattering of vague I/O variants: `ApplyIoError` is the single
typed I/O boundary, and every integrity failure has its own type above.

## 13. Historical-proposal dispositions (no accidental inheritance)

Rules the proposal detailed but ADR-0033 did not fix, each resolved
explicitly:

| Proposal rule | Disposition |
|---|---|
| Apply verification order (§5.5 steps 1–6) | **Adopted, tightened** into the §6 total order: strict parsing and tree agreement precede everything; digest/fingerprint work is delegated to the reused `verify_plan`; already-applied precedes chain equations. |
| Application-index record shape (§8.5) | **Adopted** verbatim (§5.1), with added laws the proposal left implicit: strictness, schema refusal, internal-chain validation, uniqueness refusal, no paths, deterministic rendering, and the explicit no-previous-report-field rule. |
| Report schema (§8.4) | **Adopted, modified** (§5.2): `previous_application_report_digest` added (chain self-description); `refusals` renamed `holdout_refusals` with an extended total sort tuple (core refusal kinds carry more than a `sha256`); `applied` / `unchanged` / `reviewed-unassigned` / `untouched` counts made an exact four-way partition (a `reject_suggestion` source is referenced yet unassigned, so a three-way split cannot place it); no timestamp. Rationale inline in §5.2. |
| "Byte-for-byte" preservation (§5.5) | **Modified** to the bounded, evidence-backed §10 law: byte identity for untouched files, semantic-identity + canonical rendering + fail-closed laundering guard for touched files. The literal claim is refuted by `migrate/` re-rendering and tolerant core parsing. |
| Curated-manifest path "e.g. `song-curation/manifest.json`" (§5.6) | **Adopted and fixed** (not configurable in v1): `<output>/song-curation/manifest.json`, inside the published snapshot, in a reserved area excluded from corpus enumeration; hard distinct-path refusal retained as defense in depth. |
| Report/index transactionality (§5.5, §8.5) | **Adopted, made precise** (§8.2): single commit point at the index rename; enumerated crash states; no ACID vocabulary; the registry is the sole success authority. |
| `IncompleteCoverage` as refusal under `--require-holdout-ready` (§10) | **Rejected for Slice 2**: no such mode in Apply v1; readiness is reported, not gated. Any gating belongs to the separately-accepted pilot. |
| Validation counters (§5.7) | **Adopted** as the report's `coverage` block (§5.2). |
| Redundant `supersedes_song_ids` on `accept` / `merge` / `split` (§8.2) | **Adopted as an enforced law** (§7.4): the proposal's implied equalities — `merge` ↔ its `from_song_ids`, `split` ↔ `[from_song_id]`, `accept` ↔ empty — become a per-event consistency check refusing `SupersessionEvidenceContradiction`. A digest-bound event may not carry a second, silently ignored claim about replacement rights. |
| `applied_event_ids` in the report (§8.4) | **Adopted** (batch order). |
| Index consulted at plan creation too (§8.5) | **Out of Slice-2 scope**: Slice 1's `build_plan` is frozen and takes no index; the apply-time check (§6 step 6) is the enforcement point. A planning-time convenience check may be proposed later without weakening apply. |

## 14. Preregistered RED→GREEN acceptance matrix

The later implementation must land these as genuine tests-only RED commits
first (per the repository TDD rule: failing tests committed before any
implementation, per commit, not per PR), except entries marked *(fixture)*,
which are harness/fixture work exempt from the red phase. Every test binds to
a law above; expected refusals are named.

**Plan / integrity**

| # | Case | Expected |
|---|---|---|
| A1 | valid serialized Slice-1 plan applies against its corpus and an empty index | success; output, curated manifest, report, index record all present and mutually consistent |
| A2 | plan with a foreign field at any depth | `MalformedPlanArtifact`, nothing written |
| A3 | corrupted `plan_digest` | `PlanDigestMismatch`, nothing written |
| A4 | corrupted `decisions_digest` | `DecisionDigestMismatch`, nothing written |
| A5 | corpus drifted since planning (fingerprint changed) | `PlanCorpusFingerprintMismatch`, nothing written |
| A6 | internally self-consistent forged projection (digests recomputed over forged assignments) | `DecisionProjectionMismatch`, nothing written |
| A7 | structurally invalid embedded batch (bad ordinal) plus corrupted digests | the structural refusal alone, before digest work; nothing written |
| A8 | apply-time corpus/batch faults surfacing unchanged through step 5: a corpus chunk with no `sha256`; conflicting existing labels on one source; a decision naming an unknown source; a `split` assigning one source two labels; a duplicate `event_id` in the embedded batch | the corresponding Slice-1 refusal (`UnidentifiedSource` / `ConflictingExistingSongIds` / `UnknownDecisionSource` / `SourceAssignedToMultipleSongs` / `DuplicateDecisionEventId`), nothing written |

**Chain / index**

| # | Case | Expected |
|---|---|---|
| C1 | valid initial application (empty index, `previous… == null`) | success; record appended |
| C2 | valid second application chained to the first report (all three §7.2 relations hold) | success; two ordered records |
| C3 | duplicate `batch_id` (re-apply the applied plan) | `DecisionBatchAlreadyApplied`, not `ApplicationChainMismatch` |
| C4 | wrong `previous_application_report_digest` | `ApplicationChainMismatch` naming relation (1) |
| C5 | wrong chained corpus fingerprint (batch input ≠ head output) | `ApplicationChainMismatch` naming relation (2) or the step-5 drift refusal, per which fact is broken; both variants preregistered |
| C6 | fingerprint-neutral (reject-only) batch applied, then re-applied by `batch_id` | first: success with `output == input` fingerprint and a record; second: `DecisionBatchAlreadyApplied` |
| C7 | index with a foreign field / missing index file | `MalformedApplicationIndex`, nothing written |
| C8 | index with duplicate internal `batch_id` | `DuplicateAppliedBatchId` |
| C9 | index whose adjacent records do not chain | `ApplicationIndexChainInvalid` |
| C10 | initial batch onto a fresh copy of the same corpus with its own empty index | success (independent lineage, §7.3) |
| C11 | apply started while the index lockfile exists | `ApplicationIndexLocked`, nothing written, index byte-identical |
| C12 | stale lockfile left by a crashed run *(fixture: fault injection)* | every subsequent apply refuses `ApplicationIndexLocked` until the lockfile is removed; after the §8.2 recovery, apply succeeds |

**Labels**

| # | Case | Expected |
|---|---|---|
| L1 | assign a previously unlabelled source | applied; every chunk of the source updated; counted `assignments_applied` |
| L2 | source already carrying the intended label | success; file content unchanged semantically; counted `assignments_unchanged` |
| L3 | authorized `correct` over an existing label | applied |
| L4 | authorized `merge` over existing labels | applied |
| L5 | authorized `split` of an existing label | applied |
| L6 | replacement whose on-disk label is outside the acting event's supersession set | `ExistingLabelReplacementNotAuthorized`, nothing written |
| L7 | on-disk label differs from the plan's `expected_existing_song_id` (corpus relabelled after planning) | refused as drift: `PlanCorpusFingerprintMismatch` (§7.4 note) |
| L8 | `merge` whose `supersedes_song_ids` ≠ its `from_song_ids`; `split` whose `supersedes_song_ids` ≠ `[from_song_id]` | `SupersessionEvidenceContradiction`, nothing written |
| L9 | `accept_suggestion` carrying a non-empty `supersedes_song_ids` | `SupersessionEvidenceContradiction`, nothing written |

**Filesystem**

| # | Case | Expected |
|---|---|---|
| F1 | output path equals / inside / containing the input root (incl. via symlink) | `OutputWouldModifyInput`, nothing written |
| F2 | pre-existing output path; pre-existing staging path | `OutputAlreadyExists`, nothing written |
| F3 | any refusal from steps 2–8 | no staging dir, no output, index byte-identical |
| F4 | injected write failure during staging; injected failure between publication and commit *(fixture: fault injection)* | `ApplyIoError`; filesystem in exactly one enumerated §8.2 state; index without the record ⇒ provably not applied; retry refuses `OutputAlreadyExists` |
| F5 | repeated execution from byte-identical independent input copies | byte-identical output tree, curated manifest, report, and updated index |
| F6 | index path inside a corpus tree | `ApplicationIndexInsideTree` |
| F7 | `batch_id` containing path-like content (`/`, `..`, an absolute path) | no filesystem path is influenced: staging, lockfile, and temp names are unchanged; nothing is created outside the declared roots; the apply otherwise proceeds or refuses purely by contract law |
| F8 | index supplied through a symlink alias | lock, read, temp, and commit rename all act on the canonical resolved index file; a concurrent apply through another alias of the same index refuses `ApplicationIndexLocked`; after commit the real file is replaced and every alias still resolves to the updated index |

**Corpus completeness / preservation**

| # | Case | Expected |
|---|---|---|
| K1 | a source with several chunk files (one `sha256`, many chunks) | every chunk file and the manifest record updated together |
| K2 | untouched files (unassigned chunk files, `*.group.json`, stray file) | byte-identical in the output (raw copy) |
| K3 | touched file with a member the core schema does not know | `NonCanonicalCorpusFile`, nothing published |
| K4 | root manifest with `songs: Some(..)` | `OrdinaryManifestCarriesSongs` |
| K5 | manifest/chunk-file disagreement; missing root manifest | `CorpusTreeDisagreement` |
| K6 | generated songs map vs applied labels | curated manifest `songs` equals the per-source labels' projection exactly |
| K7 | curated manifest location | at `<output>/song-curation/manifest.json`; root output `manifest.json` still has no `songs` key |
| K8 | partial curation (some sources still unlabelled) | success; `holdout_ready: false`; the unlabelled sources listed sorted in `holdout_refusals` |
| K9 | fully curated consistent fixture | `holdout_ready: true` via the real core `song_holdout_preflight` on the curated view |
| K10 | touched file whose raw text contains a duplicate JSON object key *(fixture: raw-bytes fixture)* | `NonCanonicalCorpusFile`, nothing published |
| K11 | fault-injected staged write that updates the root and curated manifests but leaves one affected `*.chunk.json` stale *(fixture: fault injection)* | `OutputPreflightInconsistent` from the staged tree-agreement re-check, nothing published |

**Report / index publication**

| # | Case | Expected |
|---|---|---|
| R1 | `report_digest` of a published report | recomputes identically under §5.4 |
| R2 | index record vs report | `batch_id`, `report_digest`, both fingerprints agree |
| R3 | after success | report exists in the published tree **and** the index record cites its digest; after any **pre-publication** refusal (steps 1–10), neither a new record nor a published tree exists; the post-publication commit-failure states (steps 11–12) are pinned by F4/R4 and the §8.2 table — a published tree without a record, provably not applied, never presented as success |
| R4 | report/index divergence cannot be presented as success | fault-injected commit failure yields an error naming the §8.2 state, never a success result *(fixture: fault injection)* |
| R5 | curated-manifest digest | `sha256sum` of the published file equals `curated_manifest_digest` |

Coverage notes: A1–A8, C1–C11, L1–L9, F1–F3, F5–F8, K1–K10, R1–R3, R5 are
RED-eligible behavioural tests. F4, R4, C12, and K11 need fault-injection
fixtures first, and K10 a raw-bytes fixture (fixture commits are not
red-phase). No silent caps: the matrix has no sampled or truncated sweeps;
every refusal in §12's **Apply-reachable surface** (the three intentionally
unreachable ledger-side members excluded, §12) appears in at least one case,
and every §6 step has at least one test crossing it.

## 15. Prior art disposition

Surveyed under the prior-art-first rule, restricted to what materially
constrains this slice. ADR-0033 already recorded the workflow-level survey;
this table records only the Slice-2 (apply/publication) deltas:

| Precedent | Reuse | Reject | Griff-specific decision |
|---|---|---|---|
| Terraform plan/apply | plan as the sole decision source; drift **refusal** at apply time | trusting the plan; external state backends | drift refusal is Slice-1 `verify_plan` reused at apply; the plan is additionally *re-derived*, not trusted (already ADR-0033 Decision 5) |
| Flyway / Alembic / Diesel migration registries | applied-**once** semantics recorded in an ordered registry consulted before applying; registry as sole authority | schema-version keys; forward-only DDL; database transactionality | the application index (§5.1): content-fingerprint-chained, JSON, strict, with internal-chain validation the migration tools leave to the database |
| git object/ref publication | content addressing (digests as identity); temp-file + `rename` as the atomic visibility switch for a single file | packfiles, reflogs, distributed refs | §8: one rename publishes the tree, one rename commits the index; the index rename is the single commit point |
| `migrate-v9` (in-repo) | `preflight_output` canonicalization and containment refusals; validate-everything-before-writing-anything; `to_string_pretty` corpus rendering | its direct-write publication (no staging — a mid-write failure leaves a partial tree at the final path) | staging + rename closes exactly the gap migrate accepted; preflight reused as specified in §6 step 1 |
| `CurationStoreV1` (in-repo) | encode-validates-what-decode-checks (a writer can never emit bytes that fail to load) — adopted for index and report serialization | nothing to reject: it is deliberately I/O-free, so it offers no publication pattern | publication semantics are defined here (§8), not borrowed |

No new dependency is required or permitted: everything above is implementable
with `std`, `serde`/`serde_json`, and the existing `griff-core` contracts
already used by Slice 1.

## 16. Implementation gate

- This contract binds nothing until **independently accepted** (ADR-0033
  Decision 10). Acceptance is a review by someone other than its author,
  recorded per repository governance at acceptance time.
- After acceptance, implementation proceeds as strict RED→GREEN against §14,
  in the accepted order (tests-only RED commits first, per commit), with the
  frozen Slice-1 suite green throughout.
- Any deviation an implementer believes necessary — a schema field, a refusal,
  an ordering change — reopens this contract for re-acceptance; it is not an
  implementation choice.
- Slice 3 (suggestions) and the controlled pilot remain gated behind their own
  separate acceptances; nothing here authorizes them. Corpus labeling of any
  real corpus remains prohibited until the pilot is independently accepted.
