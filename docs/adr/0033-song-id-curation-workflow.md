# ADR 0033: Human-confirmed, transactional song-id curation workflow

Date: 2026-07-29
Status: Proposed

## Context

ADR-0031 added a curator-assigned `SongId` (schema v10) and ADR-0032 made
song-level holdout implementable fail-closed
([`../../reachability-lab`](../../reachability-lab)). But the corpus carries no
`song_id`s, and a song holdout over an uncurated corpus correctly refuses. The
missing piece is a **way to assign `song_id`** that reduces curation labour
without letting heuristics become provenance facts by clerical accident.

The [song-id curation workflow proposal](../proposals/song-id-curation-workflow.md)
worked this out in full and survived independent review across three rounds
(artifact contracts, the ledger→plan→apply proof chain, and the batch/action
edge contracts). This ADR extracts its **durable decisions**; the proposal
remains the detailed specification. No new architecture is introduced here.

## Decision

We adopt an **offline, human-confirmed, transactional** song-id curation
workflow, owned by a standalone isolated tool. Binding decisions:

1. **Ownership.** A standalone offline `song-curation/` tool (the `fuzz` / `lab`
   / `census` / `migrate` / `reachability-lab` isolation posture, ADR-0010): not
   a production-generation dependency, no policy in the CLI/cockpit, no in-place
   corpus mutation, no automatic execution in generation. `griff-core` supplies
   only reusable schema/validation contracts (including `song_holdout_preflight`
   and the `CurationStoreV1` precedent). Not `griff curate` or `griff manifest`.

2. **Curation unit — the `sha256` source.** All chunks sharing one `sha256` are
   one indivisible source unit with one `SongId`; a source without `sha256` is
   not curatable. `title` / filename / range / track are suggestion evidence, not
   identity.

3. **Human authority; suggestions have none.** Suggestions are deterministic,
   metadata-only (v1), evidence-bearing, and never write `song_id`. No default
   action counts as acceptance; no unattended run converts suggestions into
   decisions. `SongId` is never derived from similarity (ADR-0031).

4. **Append-only batched decisions ledger** (one versioned JSON document, per
   `CurationStoreV1`). The `events` array order is the single source of truth
   (`ordinal == position`, contiguous/unique; `event_id` unique). Six first-class
   actions — accept / reject / manual_define / **correct / merge / split** — of
   which the latter three are the **authorized replacement** actions, each
   replacing only the labels in its supersession set.

5. **Plan embeds one unapplied batch; Apply proves, not trusts.** Apply consumes
   the plan (embedding the complete ordered batch) and the application index. It
   verifies `plan_digest` and `decisions_digest`, the corpus fingerprint, and the
   chain, then **replays the events, derives the assignments itself, and compares
   them** to the plan — assignments are reproduced from curator decisions, not
   asserted.

6. **Transactional application chain.** Apply writes all-or-nothing to a fresh
   output directory (never in place; `migrate-v9` output preflight), and publishes
   an application report plus an append-only **application index** record
   transactionally. It treats already-correct assignments as unchanged and
   produces deterministic output; **reuse of the same `batch_id` is not an
   idempotent re-application — it typed-refuses through the application index.**
   Incremental curation composes as batch → report → next batch, chained by report
   digest and matching input/output fingerprints; the index makes "already
   applied" provable.

7. **Deterministic identity and digests.** `SongId` is opaque, ledger-issued
   (`song-<monotonic counter>`), single-writer, issued once and never recomputed.
   Four digest contracts share one canonical JSON encoding: order-insensitive
   `corpus_fingerprint` (with a `manifest_songs` absent/present marker),
   order-sensitive `decisions_digest`, order-aware `plan_digest` and
   `report_digest`.

8. **Manifest cross-check, fail-closed.** The tool generates `CorpusManifest.songs`
   at a **distinct curated path** and refuses to target an ordinary
   `<corpus>/manifest.json`, so no ordinary `griff manifest` rebuild
   (`cli/src/main.rs`, which emits `songs: None`) can erase it. Teaching
   `griff manifest` to rebuild `songs` (strategy 2) is a later, separately
   accepted change.

9. **Fail-closed validation.** Readiness is checked with the existing core
   `song_holdout_preflight` (not a near-copy); `holdout_ready: true` only when the
   whole corpus passes it. A **valid partial snapshot** (`holdout_ready: false`,
   some sources still `None`) is a distinct state from a **complete holdout-ready
   corpus**, never conflated; every failure is a typed refusal (the proposal's
   taxonomy).

10. **Staged, gated implementation.** Four separate RED→GREEN slices —
    decision/validation core → transactional apply → suggestion generator →
    controlled pilot — each requiring **separate independent acceptance**.
    Acceptance of this ADR authorizes **slice 1 only**. The controlled pilot is
    authorized only **after slices 1–3 are accepted**; it may write
    human-confirmed `song_id` assignments **only to its small copied subset**.
    **Production / real-corpus / full-corpus labeling remains prohibited until the
    controlled pilot is independently accepted.**

## Prior art considered

Per the prior-art-first rule (AGENTS.md), we surveyed existing approaches before
designing the `ledger → plan → apply → report → application-index` pipeline. The
*identity ontology* prior art (FRBR, MusicBrainz Work/Recording/ISWC) is already
recorded in ADR-0031; that answers "what is a Work", not "how to curate it
transactionally", so this survey covers the **workflow** instead.

| Approach | What we reuse | What we reject | Decision for Griff |
|---|---|---|---|
| **Human-confirmed reconciliation** (OpenRefine clustering; MusicBrainz human-reviewed edits) | Deterministic *suggestion → explicit human confirm*; clustering as non-authoritative evidence; append-only edit history | In-place GUI mutation; online community voting; no content-hash provenance; not source-level identity | Reuse suggest-then-confirm and non-authoritative suggestions; implement offline/deterministic. |
| **Append-only event sourcing** (Fowler event logs; git commit DAG; the repo's own `CurationStoreV1`) | Immutable, ordered, append-only events; replay to derive state; per-event metadata | Distributed consensus; external event store | Reuse event-sourcing invariants natively via a `CurationStoreV1`-style ledger. |
| **Plan-before-apply with drift detection** (Terraform plan/apply; Kubernetes server-side dry-run) | A reviewable plan artifact; separate apply; fingerprint/drift **refusal** | The plan is *trusted*; external providers/state backends | Reuse plan/apply + drift-refuse, and go further: Apply **replays and re-derives** the projection rather than trusting the plan. |
| **Versioned, applied-once migrations** (Flyway / Alembic / Diesel migrations table) | Ordered, applied-**once** application recorded in a registry (≈ the application index); refuse re-apply | Forward-only DDL; no content fingerprint | Reuse the applied-registry + applied-once semantics; content-fingerprint the corpus, not a schema version. |
| **Dataset versioning and transactional histories** (DVC; lakeFS; Delta Lake) | DVC: hash-addressed data referenced through Git-versioned metadata; lakeFS: Git-like branching and commits for data; Delta Lake: ACID transaction-log commits and version lineage | Their storage engines, services, table models, and dependency footprints do not fit Griff's isolated file-based corpus tool | Reuse content identity, version lineage, and transactional publication as separate patterns; implement the Griff-specific composition natively. |

**Conclusion.** Reuse the established *patterns and invariants* above, and
implement them **natively** in the isolated Rust tool: no existing system fits
Griff's source-level `SongId`, its corpus fingerprint over `song_id` + manifest
membership, its dependency/MSRV posture, or its fail-closed typed-refusal
requirement. The novelty is only the *composition* (proof-carrying replay-derived
Apply over these known parts), not a new class of system.

## Consequences

**Good / possible.**

- `song_id` can be assigned deterministically, auditably, and transactionally,
  unblocking the ADR-0032 `HoldoutTargetSong` mode on curated corpora.
- The ledger → plan → apply → report → index chain is proof-carrying: assignments
  are replayed from curator decisions, and every declared refusal has a concrete
  evidence source.
- Suggestions cut labour without ever becoming stored identity.

**Bad / cost.**

- A new isolated tool with its own artifact schemas; not exercised by workspace
  CI (per the isolation policy, verified locally).
- Curation is human labour and can be wrong; the workflow makes errors typed and
  auditable, not impossible.

**Impossible / explicitly out of scope.**

- No automatic song assignment, similarity/cover detection, embeddings,
  classifiers, or audio analysis; no production scoring or generation change; no
  cockpit integration; no in-place mutation; no full-corpus labeling before the
  pilot gate; no track-index recovery; no arrangement identity; no change to
  ADR-0031 or ADR-0032.

Acceptance of this ADR authorizes **slice 1 only**; each later slice needs
separate independent acceptance (Decision 10). It does **not** authorize
production / real-corpus / full-corpus labeling — the controlled pilot (after
slices 1–3 are accepted) may write human-confirmed assignments **only to its
small copied subset**, and full-corpus labeling stays prohibited until the pilot
is independently accepted. The proposal is retained as the detailed specification
and historical context.
