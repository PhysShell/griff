# griff-song-curation — Slice 1: decision & validation core (ADR-0033)

An **isolated** offline tool (ADR-0010 / ADR-0033 isolation posture): deliberately
**not** a workspace member, so production builds, CI, `--workspace` clippy, the
CLI, and the cockpit never acquire curation policy. `griff-core` supplies only
reusable schema/validation contracts.

This crate implements **Slice 1** of the accepted ADR-0033 workflow: the
**read-only decision & validation core**. It reads synthetic/fixture inputs and
**writes no corpus** — there is no apply path here.

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
- `validate_batch(&DecisionBatch)` — array order is authoritative, `ordinal` ==
  position, `event_id` unique within the batch.
- `validate_ledger(&DecisionsLedger)` — the above per batch, **plus `event_id`
  uniqueness across all batches**.
- `build_plan(&CorpusManifest, &DecisionsLedger, batch_id) -> Result<DryRunPlan, …>`
  — validate the whole ledger, select the batch, replay it (latest event for a
  `sha256` wins), embed the complete ordered batch, and derive:
  - **assignments** — the sources the batch labels (`reject_suggestion` assigns
    nothing; a later `reject` supersedes an earlier pending assignment;
    `correct` / `merge` / `split` are the authorized replacements);
  - **`generated_songs_map`** — the **complete post-plan** state: every source's
    final label (the batch assignment when present, else its existing
    authoritative label), so **untouched existing labels survive** into the map
    that becomes the Slice-2 manifest projection.
- `verify_plan(&DryRunPlan, &CorpusManifest)` — recompute both digests and
  **re-derive the whole contract** (top-level fingerprint, batch fingerprint,
  schema/policy, assignments, and complete songs map) from the embedded events.

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
`DuplicateDecisionEventId`, `BatchNotInLedger`,
`DecisionBatchFingerprintMismatch`, `DecisionDigestMismatch`, `PlanDigestMismatch`,
`DecisionProjectionMismatch`.

## Out of scope (later, separately-accepted slices)

No corpus writes, output tree, transactional apply, application report/index,
manifest generation, holdout-readiness execution over a changed corpus,
suggestion generation or metadata normalization, interactive confirmation UI,
CLI/cockpit integration, or any real corpus files/labels. Slices 2–4 each need
separate independent acceptance; corpus labeling stays prohibited until the
controlled pilot is independently accepted (ADR-0033 Decision 10).

## Run

```sh
cargo test   --manifest-path song-curation/Cargo.toml
cargo clippy --manifest-path song-curation/Cargo.toml --all-targets
cargo fmt    --manifest-path song-curation/Cargo.toml -- --check
```

Isolated crate → **not exercised by workspace CI** (ADR-0010 precedent); verified
locally under `nix develop`.
