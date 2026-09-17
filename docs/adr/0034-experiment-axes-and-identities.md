# ADR 0034: Run generator experiments as variant × information regime, with separate identities and comparable-only metrics

Date: 2026-09-17
Status: Proposed

## Context

The S8 cockpit compares exactly one pair: S6 Intact against S7 Global Chain,
both taken from one Generate run. Future work needs the same comparison for many
policies (fingering, chord voicing, k-best, harmonic scoring). For each, it
needs to know whether an improvement survives without the corpus, and which
corpus channel carries it.

The corpus is not one switch. `CorpusMaterial` supplies three independent
channels: rhythm templates, novelty references, and a gesture.

What a pass *actually* took already differs from what was attached, which is
why `CorpusContribution` exists.

Two of the numbers a comparison would reach for depend on the regime:
- novelty is measured against the references the pass was given;
- the S7 chain cost is built from S6 aggregates that include novelty.

A cross-regime difference in them therefore partly measures the measuring
stick changing.

ADR-0032 (Accepted) puts holdout and population selection in the offline
Reachability Lab and forbids holdout policy in the CLI and cockpit.

The design note
[`../proposals/generator-observatory.md`](../proposals/generator-observatory.md)
§8 records the review that shaped this decision. The in-memory form below is
implemented and tested (`experiment/`, `core/tests/corpus_material_view.rs`,
`core/tests/generation_input_characterization.rs`).

## Decision

1. **A separate library owns experiments.** `griff-experiment` (`experiment/`)
   depends on `griff-core`; the CLI and the cockpit depend on it.
   - Orchestration, identities, metric comparability and (later) the bundle
     schema live there, not in the canonical model.
   - `griff-core` gains only production-neutral generation seams: the borrowed
     `CorpusMaterialView`, `ranked_candidates_from_view` (which the public
     `ranked_candidates` wraps, output-identical to main), and the pass-level
     `CorpusContribution`.

2. **Two axes.**
   - An *algorithm variant* (`VariantSpec`) is a typed, statically registered
     choice per pipeline stage: generator, scorer, selector, realizer, with a
     reserved validator slot. Each policy has a `PolicyIdentity { id, version }`.
   - An *information regime* (`InformationRegime`) is a mask over the three
     channels of an already prepared population.
   - A new experiment is a new policy arm and its adapter in the runner. No
     frontend matches on a variant.

3. **Regimes mask; the Lab selects.** The layers stay in this order:
   holdout / population selection (Reachability Lab only, ADR-0032) → prepared
   population → information regime → channel masking.
   - The Observatory never selects, filters or splits a population, and never
     calls one a valid holdout.
   - `FULL` means every channel of the bound population. It is neither an
     evaluation corpus, nor `LeakyDiagnostic`, nor a holdout.
   - A bundle may carry facts a caller supplies (population identity, a
     Lab-produced leak check), never a verdict of its own.

4. **Four identities, never one.** They are separate fingerprints:
   - the experiment spec: ask, variants' stage identities, regimes, evaluation
     context;
   - the bound population snapshot: per channel and whole;
   - what a generation pass could consume;
   - the evaluation context.

   A cell's recipe is its pass's information plus its selector and realizer
   identities.

   A pass's information hashes each channel *as offered*: masked or absent
   channels read as empty. A cell's identity therefore depends only on the
   channels it could consume, while the run separately records the whole
   bound snapshot.

5. **Fingerprint contract.**
   - SHA-256 over a domain-tagged, length-prefixed walk of the canonical model.
     Sequences carry their length, floats hash by bits, and enums hash by
     exhaustive matches.
   - Walks destructure public model types exhaustively, so a new field or
     variant is a compile error.
   - Three facts are reached through accessors because their fields are
     private, so the compiler does not guard them: `Tempo`, `Tuning` and
     `NoteMarks`.
   - The tonal context hashes through its own serde projection.
   - Palette and reference order are part of identity, because order is
     behaviour.
   - Paths, run and history ids, timestamps and UI state are never hashed.
   - A persisted form must reproduce these fingerprints; it does not define its
     own.

6. **Metrics carry comparability identity.** Every value carries
   `MetricIdentity { kind, name, owner, context }`.
   - An **evaluation** is measured by a fixed evaluator in the spec's explicit
     evaluation context. The context is `EvaluationContext::None` or a supplied
     context with its own fingerprint, and is never defaulted to the runtime
     corpus.
   - A **policy objective** is a policy's own number. Its context is the
     information of the pass that produced it.
   - A delta exists only between identical identities.
   - An interaction `(B1 − A1) − (B0 − A0)` exists only over four evaluations
     with one identity.
   - Everything else is `Comparison::Unavailable` with a typed reason (missing,
     incompatible identity, not an evaluation). A frontend shows no number for
     it.
   - No aggregate "quality" score is introduced.

7. **One pass per information need, shared; nothing regenerates.**
   - Within a regime, variants with equal generator and scorer share one ranked
     set. The chain is planned at most once per pass.
   - Refusals are typed cell outcomes, never fake results.
   - A result's `realization` is an uninhabited type until a realizing policy
     exists. Nothing fabricates one.
   - Opening, switching, auditioning or exporting a recorded result never runs
     a generator or a planner. The projection's inputs cannot express one.

8. **The bundle is versioned and waits for this ADR.** No persisted experiment
   schema is published before acceptance. The bundle then:
   - is a versioned wire mirror (`…V1` types) with explicit conversions, not
     serde on the canonical `Score`;
   - is lossless on the represented subset, and any field it does not keep is
     listed, refused with a typed error, or proven irrelevant to replay and
     display;
   - carries schema id and version, spec, source, population and evaluation
     identities, passes and cells.

   The frontend-owns-the-wire-format precedent of the Global Chain Audition
   `decisions.log` entry does not apply: the CLI writes bundles and the cockpit
   reads them, so the wire types are shared.

Prior art (reused as ideas; no code or dependency):
- **Content- vs input-addressed stores.** Nix and Bazel action keys hash a
  recipe's inputs apart from its output's content. That is the recipe vs
  content split here.
- **Data-versioned experiment tracking** (DVC / MLflow). Parameters, data
  dependencies and metrics are recorded by content hash, and a run is a record,
  not a re-execution.
- **2 × 2 factorial designs.** The interaction contrast of two factors is the
  difference of simple effects, defined only on one measurement scale.

## Consequences

**Good / possible.**
- A research cycle becomes:
  1. register a policy;
  2. run variants × regimes headlessly;
  3. inspect effects and failure cases, with the channel that carries an
     effect named rather than guessed.
- "The corpus helped" becomes testable per channel, and "the algorithm helped
  without a corpus" becomes a direct contrast.
- Two runs over populations that differ only in references keep identical
  seed-only, rhythm-only and gesture-only cells.
- A number that would silently mix scales cannot be computed through the API.

**Bad / cost.**
- One more workspace crate, and identity versions for the stages the
  experiment crate names.
  - The scorer identity is pinned to the core rerank policy by a test.
  - The generator and evaluator identities are this crate's promise, bumped by
    hand when behaviour changes.
- One pass per requested regime: two regimes whose offered views coincide (for
  example `FULL` without a population and `SEED_ONLY`) still generate twice.
  That is an explicit, bounded redundancy, kept so every pass has one requested
  regime.
- Cross-regime evaluation needs an explicitly supplied context. Without one,
  interactions are unavailable by design.

**Impossible / out of scope.**
- The following in the Observatory, CLI or cockpit:
  - holdout, population selection or train/test split policy;
  - a `use_corpus` flag;
  - dynamic plugins or runtime scripting;
  - solver or ML runtimes.
- A default evaluation corpus, and cross-scale deltas.
- A TAB or realization view before a realizing client exists.
- Leakage facts before the Lab supplies them.
- Headless-vs-cockpit equality before both corpus loaders accept the same
  records (PhysShell/griff#203).
