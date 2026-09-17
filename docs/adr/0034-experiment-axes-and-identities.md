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
implemented and tested:
- `experiment/`;
- `core/tests/corpus_material_view.rs`;
- `core/tests/generation_input_characterization.rs`.

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
     Lab-produced leak check), never a verdict of its own. A holdout the Lab
     refused is never recorded as one that happened.

4. **Separate identities, never one.**
   - **Experiment spec:** ask, variants' stage identities, regimes, evaluation
     context.
   - **Bound population snapshot:** per channel and whole.
   - **Information a generation pass could consume:** each channel *as
     offered*, so masked or absent channels read as empty.
   - **Evaluation context.**
   - **A cell's requested identity** (`Cell::requested`): source, ask, every
     stage identity, the *requested* regime, and the bound population's
     identity or its absence. The evaluation context is not part of it.
   - **A cell's effective identity** (`Cell::recipe`): its pass's information
     plus its selector and realizer identities.

   **Requested and effective identities never collapse.** `FULL` over no
   population and `SEED_ONLY` have equal recipes and different requests, and
   remain two cells: asking for a corpus and getting nothing is itself a
   finding. Equal effective identities may share *execution* in the future,
   but only as memoization keyed by the recipe, never as normalization of the
   request (`FULL → SEED_ONLY`).

   A cell's recipe depends only on the channels it could consume, while the run
   separately records the whole bound snapshot. Together they answer "which
   snapshot?" and "which parts of it could have influenced this cell?".

5. **One canonicalization of the score, versioned.**
   - Fingerprints are SHA-256 over a domain-tagged (`griff.score.v1`, …),
     length-prefixed walk of the canonical model. Sequences carry their
     length, floats hash by bits, enums hash by exhaustive matches, and public
     model types are destructured exhaustively.
   - Palette and reference order are part of identity, because order is
     behaviour.
   - Paths, run and history ids, timestamps and UI state are never hashed.
   - When the bundle introduces its wire types, it introduces **one** canonical
     semantic projection (`…V1`) of the score and the other model values. Both
     the bundle serialization and the fingerprint derive from that single
     projection. A second, independent canonicalization is not allowed.
   - Moving the fingerprint onto the projection either reproduces the pinned
     v1 goldens (`experiment/tests/identity_pins.rs`) or bumps the domain
     version together with the golden.
   - Some facts are reached through accessors, because their fields are
     private: `Tempo`, `Tuning`, `NoteMarks`. Likewise the tonal context
     hashes through its own serde projection. This is an accepted property
     of a versioned projection, not a reason to expose the canonical model's
     internals.
   - **Rule:** any change to what the projection observes requires a domain
     version bump and a golden change in the same edit.

6. **Version ownership follows the semantics.** A policy's identity lives next
   to the implementation it identifies, and the experiment crate consumes it.
   - Read from core today:
     - the scorer (`rerank::rerank_weights_v1`);
     - the global-chain selector (`candidate_chain::chain_weights_v1`).
   - Owned by `griff-experiment`, because the semantics are its own:
     - the `generation_axes` evaluator;
     - the `no_realization` realizer.
   - **Debt, manual contract:** the S6 candidate-set generator
     (`s6_candidate_set` v1) and the intact selection (`intact_top` v1). Core
     carries no identity for either yet. Each is pinned by a characterization
     golden sharing one assertion with its version, so a behaviour change
     fails next to the number that must change. Updating only the golden is
     the one wrong fix.
   - The debt is retired by giving `griff-core` those identities before any
     second generator or selector arm is registered.
   - The Observatory never assigns a version to a policy it does not own
     without such a pin.

7. **Metrics carry comparability identity.** Every value carries
   `MetricIdentity { kind, name, owner, context }`.
   - An **evaluation** is measured by a fixed evaluator in the spec's explicit
     evaluation context. The context is `EvaluationContext::None` or a supplied
     context with its own fingerprint, and is never defaulted to the runtime
     corpus.
   - A **policy objective** is a policy's own number. Its context is the
     information of the pass that produced it. It is not automatically
     comparable even within one regime: two scorers may live on two scales.
   - A delta exists only between identical identities.
   - An interaction `(B1 − A1) − (B0 − A0)` exists only over four evaluations
     with one identity.
   - Everything else is `Comparison::Unavailable` with a typed reason (missing,
     incompatible identity, not an evaluation). A frontend shows no number for
     it.
   - No aggregate "quality" score is introduced.

8. **One pass per information need, shared; nothing regenerates.**
   - Within a regime, variants with equal generator and scorer share one ranked
     set. The chain is planned at most once per pass.
   - Refusals are typed cell outcomes, never fake results.
   - A result's `realization` is an uninhabited type until a realizing policy
     exists. Nothing fabricates one.
   - Opening, switching, auditioning or exporting a recorded result never runs
     a generator or a planner. The projection's inputs cannot express one.

9. **The bundle is versioned and waits for this ADR.** No persisted experiment
   schema is published before acceptance. The bundle then:
   - serializes the canonical projection of decision 5, not serde on the
     canonical `Score`;
   - is lossless on the represented subset, and any field it does not keep is
     listed, refused with a typed error, or proven irrelevant to replay and
     display;
   - carries schema id and version, spec, source, population and evaluation
     identities, passes, and cells with both their requested and effective
     identities.

   The frontend-owns-the-wire-format precedent of the Global Chain Audition
   `decisions.log` entry does not apply: the CLI writes bundles and the cockpit
   reads them, so the wire types are shared.

**Characterization evidence (not a performance contract).** All eight regimes ×
{S6 Intact, S7 Global Chain} were run over one repository-corpus source (9,686
references, 6,146 rhythm templates, one gesture):
- 16 cells were produced by 8 generation passes;
- no cell was refused;
- a replay was identical;
- the S6 → S7 chain-cost delta was available within a regime and
  `IncompatibleIdentity` across regimes;
- interactions existed only for evaluation axes.

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
  seed-only, rhythm-only and gesture-only recipes. Their requests still record
  that the population differed.
- A number that would silently mix scales cannot be computed through the API.

**Bad / cost.**
- One more workspace crate.
- Two production identities are manual contracts until core owns them. They
  are pinned, but they still need a person to bump the version when a golden
  breaks.
- One pass per requested regime: two regimes whose offered views coincide
  still generate twice. That is a measured, bounded cost of simplicity, to be
  removed only by recipe-keyed memoization.
- Cross-regime evaluation needs an explicitly supplied context. Without one,
  interactions are unavailable by design.

**Impossible / out of scope.**
- The following in the Observatory, CLI or cockpit:
  - holdout, population selection or train/test split policy;
  - a `use_corpus` flag;
  - dynamic plugins or runtime scripting;
  - solver or ML runtimes.
- A default evaluation corpus, cross-scale deltas, request normalization, and a
  second score canonicalization.
- A TAB or realization view before a realizing client exists.
- Leakage facts before the Lab supplies them.
