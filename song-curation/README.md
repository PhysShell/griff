# griff-song-curation — ADR-0033 Slices 1–2

An **isolated** offline tool (ADR-0010 / ADR-0033 isolation posture): deliberately
**not** a workspace member, so production builds, CI, `--workspace` clippy, the
CLI, and the cockpit never acquire curation policy. `griff-core` supplies only
reusable schema/validation contracts.

This crate implements **Slice 1** of the accepted ADR-0033 workflow — the
**read-only decision & validation core** (accepted and frozen) — and
**Slice 2**, the **transactional Apply** (`apply` module), implemented
against the independently accepted Slice-2 contract
([`../docs/proposals/song-curation-slice-2-transactional-apply.md`](../docs/proposals/song-curation-slice-2-transactional-apply.md),
normative reviewed artifact `47e734c`; acceptance recorded in
`docs/decisions.log.md` @ `bad7b44`). Implementation evidence:
[`../docs/audit/2026-08-slice2-apply-implementation.md`](../docs/audit/2026-08-slice2-apply-implementation.md).

## What Slice 1 does

```text
inventory (collapse chunks by exact sha256)
  → validate the whole batched decisions ledger
  → project one unapplied batch into the complete post-plan source → song state
  → serialize a dry-run plan that embeds the complete ordered batch
  → verify that plan by re-deriving its whole contract
```

Public API:

- `inventory(&CorpusManifest) -> Result<Inventory, Vec<CurationError>>` — collapse
  by `sha256` into sorted/unique `SourceRecord`s; refuse `UnidentifiedSource`
  (no `sha256`) and `ConflictingExistingSongIds` (one `sha256`, two `song_id`s).
- `corpus_fingerprint(&CorpusManifest) -> String` — order-insensitive, over
  canonical-JSON records with a `manifest_songs` absent/present marker.
- `decisions_digest(&DecisionBatch)` / `plan_digest(&DryRunPlan)` — order-sensitive
  / order-aware, under one shared canonical JSON encoding (object keys sorted,
  compact UTF-8, array order preserved); `plan_digest` omits its own field.
- `validate_batch(&DecisionBatch)` — a standalone batch: array order is
  authoritative, `ordinal` == position, `event_id` unique within the batch.
- `validate_ledger(&DecisionsLedger)` — the ledger's own identity and structure:
  the ledger `schema` is `song-curation.decisions.v1`
  (`UnsupportedDecisionsLedgerSchema`), `batch_id`s are **unique**
  (`DuplicateDecisionBatchId`, so batch selection is unambiguous), positional
  ordering per batch, **plus a single ledger-wide `event_id` uniqueness pass** so
  a duplicate (intra- or cross-batch) is reported **exactly once**, never twice.
  `next_song_seq` is read but **not** enforced here — song-id issuance is a later
  slice.
- `build_plan(&CorpusManifest, &DecisionsLedger, batch_id) -> Result<DryRunPlan, …>`
  — validate the whole ledger **and short-circuit on structural invalidity
  before any batch selection or projection** (so a schema / duplicate-id /
  ordering fault never reaches fingerprinting or replay, and is emitted exactly
  once), then select the unique batch, replay it (latest event for a `sha256`
  wins), embed the complete ordered batch, and derive:
  - **assignments** — the sources the batch labels (`reject_suggestion` assigns
    nothing; a later `reject` supersedes an earlier pending assignment;
    `correct` / `merge` / `split` are the authorized replacements);
  - **`generated_songs_map`** — the **complete post-plan** state: every source's
    final label (the batch assignment when present, else its existing
    authoritative label), so **untouched existing labels survive** into the map
    that becomes the Slice-2 manifest projection.
- `verify_plan(&DryRunPlan, &CorpusManifest)` — validate the embedded batch
  **first** and **short-circuit**: an invalid ordinal or duplicate `event_id`
  refuses before any digest, fingerprint, or replay work. Otherwise recompute
  both digests and **re-derive the whole contract** (top-level fingerprint, batch
  fingerprint, schema/policy, assignments, and complete songs map) from the
  embedded events. The two fingerprint checks are **separate**:
  `PlanCorpusFingerprintMismatch` (top level) and
  `DecisionBatchFingerprintMismatch` (embedded batch) each report the value of
  the field that actually disagreed, never a substitute.

## The artifact is JSON, and the verifier reads it back

The whole ledger/plan graph is **strict on deserialization**: `DryRunPlan`,
`Assignment`, `DecisionsLedger`, `DecisionBatch`, `DecisionEvent`, `SplitTarget`,
and the internally tagged `Action` all carry `#[serde(deny_unknown_fields)]`, so
a foreign field at **any** depth is rejected — not silently dropped before the
digests are recomputed over a laundered object. (For the internally tagged
`Action`, a regression test parses a rogue field inside a variant payload and
asserts the actual serde version refuses it; the `kind` tag is still accepted.)
The plan is an **immutable serialized artifact** (ADR-0033), so it round-trips
build → serialize → deserialize → `verify_plan`, and the verifier operates at
that artifact boundary — not merely on an already-constructed Rust struct —
which is the input contract Slice 2's apply path will consume.

## The load-bearing guarantee

`verify_plan` does not trust the plan's own assertions. Even a plan whose
`plan_digest` and `decisions_digest` are internally self-consistent is refused if
re-deriving from its embedded events + the corpus does not reproduce its
`assignments` / `generated_songs_map` (`DecisionProjectionMismatch`), or if its
top-level `input_corpus_fingerprint` / schema / policy contradicts the corpus and
tool. A plan cannot rubber-stamp itself — assignments are *reproduced from
curator decisions*, not asserted.

## Refusals (Slice-1 subset)

`UnidentifiedSource`, `ConflictingExistingSongIds`, `UnknownDecisionSource`,
`SourceAssignedToMultipleSongs`, `InvalidDecisionBatchOrder`,
`DuplicateDecisionEventId`, `DuplicateDecisionBatchId`,
`UnsupportedDecisionsLedgerSchema`, `BatchNotInLedger`,
`PlanCorpusFingerprintMismatch`, `DecisionBatchFingerprintMismatch`,
`DecisionDigestMismatch`, `PlanDigestMismatch`, `DecisionProjectionMismatch`.

## Slice 2: transactional Apply (`apply` module)

`apply(&ApplyPaths) -> ApplyRun` consumes a **serialized** Slice-1 plan,
the current corpus snapshot (a directory tree), and the application index,
and — under the contract's 12-step fail-closed verification order —
publishes the curated snapshot with its proof artifacts: the curated
manifest at the protected `song-curation/manifest.json` path, the
application report, and the appended `song-curation.applications.v1` index
record. **The batch is applied iff its record is in the index**: one
publication `rename` makes the snapshot visible; the index temp+`rename`
under a marker-bearing single-writer lock is the single commit point, and
every refusal — the closed 24-member typed surface plus the Slice-1
refusals reused verbatim through `verify_plan` — proves the run did not
commit. Untouched files are raw byte copies; touched JSON is rewritten
under a fail-closed laundering guard (duplicate-key pass + round-trip
equality); the real core `song_holdout_preflight` runs exactly once, over
the staged curated view.

## Out of scope (later, separately-accepted slices)

No suggestion generation or metadata normalization, no interactive
confirmation UI, no CLI/cockpit integration, and **no real corpus
files/labels** — every input this crate has ever touched is a synthetic
fixture. Slices 3–4 each need separate independent acceptance; corpus
labeling stays prohibited until the controlled pilot is independently
accepted (ADR-0033 Decision 10).

## Run

```sh
cargo test   --manifest-path song-curation/Cargo.toml
cargo clippy --manifest-path song-curation/Cargo.toml --all-targets
cargo fmt    --manifest-path song-curation/Cargo.toml -- --check
```

Isolated crate → **not exercised by workspace CI** (ADR-0010 precedent); verified
locally under `nix develop`.
