# Proposal: Active Preference Selection

Use limited human feedback to choose the most informative candidate comparisons,
without pretending that musical preference is an objective class label.

Status: proposal for discussion
Scope: research and evaluation design adjacent to S9. No production reranking,
sampling, generator, scorer, or cockpit behaviour changes. No roadmap stage is
assigned by this proposal.

## 1. Goal

Determine whether Griff can learn a useful, inspectable preference policy with
fewer user judgements by choosing **which candidate comparison to ask for next**.

The target is not a universal "best generator". The target is a contextual
belief such as:

- which strategy or configuration is most promising for the current request;
- which of two already-valid candidates is more informative to compare;
- how uncertain Griff remains, and which evidence changed that belief.

The first deliverable is an offline, deterministic replay experiment. An
interactive acquisition policy is out of scope until replay evidence shows a
material benefit over simpler S9 baselines.

## 2. Why this is not ordinary feedback collection

S8 already records favorite / reject / history / provenance for generated and
Swang candidates. S9 plans inspectable feedback events and a simple explainable
reranking baseline. Those are prerequisites, not substitutes, for active
selection.

A passive system shows whatever the current ranking produced and records the
reaction. An active system asks a narrower question:

> Among the valid comparisons available now, which answer would reduce the
> uncertainty about the user's contextual preference the most?

That distinction matters because user attention is more expensive than CPU.
Randomly accumulating reactions can produce a large event log while leaving the
important strategy boundaries almost unobserved.

It also creates new risks: a policy that chases uncertainty can repeatedly show
odd edge cases, overexpose one strategy, or optimize for informative questions
rather than useful music. Existing validators and quality guards therefore
remain hard gates around every candidate pool.

## 3. Prior art: CODA

The immediate prior art is Justin Kay et al., **Consensus-Driven Active Model
Selection (CODA)**, an ICCV 2025 Highlight:

- paper: <https://arxiv.org/abs/2507.23771>
- reference implementation: <https://github.com/justinkay/coda>

CODA addresses a classification problem. Given predictions from many fixed
models over the same unlabelled examples, it:

1. uses agreement between candidate models to initialize probabilistic
   confusion-matrix beliefs;
2. maintains a posterior probability that each model is best;
3. simulates the possible labels for every candidate example;
4. asks for the label with the highest expected information gain;
5. updates the posterior and repeats.

The useful idea is the **decision loop**: posterior over the current winner,
hypothetical updates, then acquisition by expected reduction in uncertainty.

The classification model itself does not transfer to Griff:

- Griff candidates are different musical objects, not classifier predictions
  for one shared object;
- a favorite is a preference observation, not an objective ground-truth class;
- the preferred strategy may change with request, gesture, register, tonal
  context, density, and session;
- several candidates may be acceptable, tied, skipped, or rejected together;
- scorer consensus can share one systematic musical bias.

Therefore this proposal adopts the **idea**, not CODA's Dirichlet confusion
matrices or Python implementation. No code reuse is proposed. At the time of
this survey the reference repository did not expose a `LICENSE` file, and the
required preference model is different anyway.

## 4. Proposed Griff formulation

### 4.1 Observation unit

The primary future observation is a contextual comparison:

```rust
struct PreferenceComparison {
    comparison_id: ComparisonId,
    session_id: SessionId,
    generation_run_id: GenerationRunId,
    left_candidate_id: CandidateId,
    right_candidate_id: CandidateId,
    outcome: ComparisonOutcome,
    context_snapshot_id: ContextSnapshotId,
    acquisition_policy: PolicyIdentity,
}

enum ComparisonOutcome {
    Left,
    Right,
    Tie,
    RejectBoth,
    Skip,
}
```

These names are illustrative only. S9 owns the canonical event vocabulary and
storage contract. This proposal must not silently create a competing feedback
schema.

Existing unary favorite / reject events remain useful for the first baseline.
They must not be fabricated into pairwise wins unless the derivation rule is
explicit, versioned, and replayable. In particular, absence of a favorite is
not a loss, and `Skip` is not negative feedback.

### 4.2 Context

Every observation must be tied to an immutable context snapshot containing or
referencing the facts available when the choice was shown:

- generation request and constraints;
- candidate and source fingerprints;
- strategy / configuration identities and seeds;
- named rerank axes and their policy identity;
- structure, rhythm, register, playability, tonal, and gesture facts that were
  actually available at that revision;
- candidate order and presentation surface;
- session and generation lineage.

Raw mutable "current profile" state is not provenance. Replay must reconstruct
why a comparison was selected from captured inputs plus versioned policy code.

### 4.3 Preference belief

The minimal belief is conditional, not global:

```text
P(strategy or candidate is preferred | request context, observed feedback)
```

Candidate model families, in increasing complexity:

1. **Beta-Bernoulli strategy baseline** over unary approve / reject observations,
   with explicit treatment of favorite, tie, reject-both, and skip semantics.
2. **Bradley-Terry-Luce or Thurstone pairwise model** over strategy or
   configuration utility.
3. **Contextual pairwise model** using a deliberately small, versioned feature
   set after the non-contextual model is understood.

A neural preference model, reinforcement learning, and opaque embedding-only
utility are out of scope. S9 explicitly requires an inspectable profile and
prohibits jumping to gradient descent / RL before the simple baseline exists.

### 4.4 Acquisition policy

For a captured candidate panel, the acquisition policy may rank eligible pairs
by expected reduction in uncertainty about the contextual winner.

Conceptually:

```text
current posterior over preferred strategy/configuration
→ simulate each allowed response to each eligible pair
→ update the posterior hypothetically
→ measure expected entropy or expected simple-regret reduction
→ choose the best pair under deterministic tie-breaks
```

Expected information gain is one candidate objective, not a foregone decision.
Expected simple-regret reduction may align better with the actual question:
selecting a useful candidate, not merely producing a well-calibrated posterior.
Phase 0 must compare the acquisition objectives before one becomes canonical.

The pair pool is constrained before acquisition:

- both candidates already passed the normal structural validators;
- no candidate is generated solely because it is statistically confusing;
- minimum quality / playability guards are inherited from the captured run;
- duplicate or musically equivalent candidates are excluded by an explicit
  equivalence policy;
- pair ordering is randomized or counterbalanced by a deterministic policy so
  presentation bias can be measured.

## 5. Determinism and provenance

Active selection changes what the user sees, so its evidence contract must be
stricter than an offline scorer experiment.

A replay identity includes at least:

```text
candidate panel snapshot
feedback prefix
preference-model id + version
feature-policy id + version
acquisition-objective id + version
tie-break policy
seed, when stochasticity is admitted
```

For fixed inputs, the selected pair, posterior summary, explanation, and next
state must be identical. Any stochastic exploration is explicit, seeded, and
recorded. Hidden wall-clock state, insertion-order dependence, and mutable
session-global caches are forbidden determinants.

Each recommendation must be explainable as named facts, for example:

```text
asked A vs B because both remain plausible winners for this context;
their rhythmic-density and register profiles differ materially;
the possible answers separate the posterior more than the other eligible pairs
```

An entropy number alone is not a user-facing explanation.

## 6. Evaluation design

Logged passive feedback is not sufficient by itself for honest counterfactual
claims: it contains outcomes only for candidates the old policy happened to
show. The benchmark therefore has two evidence tracks.

### 6.1 Synthetic-oracle track

Construct deterministic preference oracles over known candidate features and
context interactions. This proves:

- posterior updates are mathematically and numerically correct;
- acquisition is deterministic;
- the policy recovers known winners;
- context-dependent preferences do not collapse into one global strategy;
- tie, reject-both, skip, and missing-feedback semantics are preserved.

Synthetic success is necessary but never musical evidence.

### 6.2 Complete-panel human track

Collect bounded evaluation sessions in which the user evaluates a complete
captured panel or a predeclared comparison set. Hide most judgements during
replay, reveal them only when a simulated policy asks, and compare policies on
the same immutable evidence.

This avoids claiming that an unobserved comparison would have had a convenient
answer. It also makes random, passive-order, uncertainty, and information-gain
policies directly comparable.

### 6.3 Baselines

At minimum:

1. random eligible pair;
2. original panel / ranking order;
3. simple uncertainty sampling;
4. non-contextual Beta or pairwise preference model;
5. the proposed active objective.

The strongest simple baseline, not random alone, is the promotion comparator.

### 6.4 Metrics

Report named measures rather than one celebratory aggregate:

- probability of identifying the complete-panel winner after `k` answers;
- simple regret after `k` answers;
- cumulative regret of candidates shown during the session;
- posterior calibration where the model exposes probabilities;
- number of questions to a predeclared decision-confidence threshold;
- strategy and context exposure balance;
- performance by sparse / minority context slices;
- skip, tie, reject-both, and abandonment rates;
- wall-clock acquisition cost separately from user-question count.

Thresholds and the primary metric must be registered before running the final
comparison. The proposal deliberately does not invent a percentage that the
benchmark has not justified.

## 7. Evidence-gated phases

### Phase 0 — audit and benchmark contract

No production code.

Deliverables:

- inventory the exact S8 feedback, history, candidate, run, and provenance facts;
- identify what S9 Phase 0 must add for immutable replay;
- define unary and pairwise outcome semantics without competing vocabulary;
- specify candidate-panel capture and equivalence rules;
- compare posterior and acquisition objectives on paper;
- define synthetic oracles, complete-panel protocol, baselines, metrics, and
  promotion thresholds;
- record CODA and preference-learning prior art, including failure modes and
  licensing posture.

### Phase 1 — deterministic offline replay harness

- immutable panel + feedback-prefix inputs;
- policy identity and deterministic tie-breaks;
- synthetic-oracle fixtures;
- random and passive-order baselines;
- machine-readable per-step trace and summary;
- no cockpit integration and no production ranking changes.

### Phase 2 — simplest preference posterior

- unary Beta baseline first;
- non-contextual pairwise model only after pairwise evidence exists;
- calibration and winner-recovery tests;
- inspectable posterior summaries;
- benchmark against the existing S9 EMA baseline rather than replacing it by
  assumption.

### Phase 3 — active acquisition

- hypothetical posterior updates;
- EIG and/or expected-regret acquisition behind an experiment boundary;
- bounded candidate-pair prefilter with measured cost;
- offline replay against all baselines;
- context and exposure-bias slices reported before any promotion discussion.

### Phase 4 — bounded cockpit experiment

Opens only if Phase 3 materially reduces human questions against the strongest
simple baseline without degrading minority contexts or repeatedly surfacing
low-value candidates.

The first UI experiment is explicit opt-in and session-local. It does not alter
default generation, production reranking, or saved preference profiles until a
separate acceptance contract is approved.

## 8. Promotion gates

No active policy reaches product behaviour unless all gates pass:

1. **Contract gate:** S9 has stable feedback, candidate, session, run, and context
   provenance with `Skip` distinct from negative feedback.
2. **Replay gate:** fixed snapshots reproduce pair choice, posterior, explanation,
   and trace byte-for-byte or structurally under a declared canonical format.
3. **Correctness gate:** synthetic oracles recover known context-dependent
   winners and exercise every outcome semantic.
4. **Baseline gate:** active selection beats the strongest simple baseline on
   the preregistered primary metric across held-out complete-panel sessions.
5. **Slice gate:** gains do not come from sacrificing sparse strategies or
   minority request contexts; failures are reported by named slice.
6. **Burden gate:** skip / abandonment and repeated-comparison rates show that
   information gain is not purchased by making the cockpit annoying.
7. **Explanation gate:** each query and recommendation carries inspectable named
   reasons and versioned provenance.
8. **Scope gate:** promotion receives a separate S9 acceptance contract or ADR;
   this proposal alone authorizes nothing.

## 9. Failure modes to test explicitly

### Shared-bias consensus

CODA can be misled when candidate models share a class or distribution bias.
The Griff analogue is treating agreement among rerank axes, heuristics, or
strategies as musical truth. Consensus may initialize a prior, but human
feedback must be able to overturn it, and scorer-derived facts must remain
separate from preference observations.

### Exposure bias

A strategy cannot receive positive feedback if the current reranker rarely
shows it. Complete-panel evaluation and exposure metrics are required before
interpreting observed favorite rates as utility.

### Context confounding

A strategy may appear weak only because it is used for harder requests. Global
win counts and unconditioned Beta priors can encode this confounding as a false
preference.

### Non-stationary preference

User preference may drift within or across sessions. The model must distinguish
session-local evidence from an intentionally persistent profile, and any decay
or reset policy must be explicit and replayable.

### Informative but bad questions

Maximum-entropy pairs can be musically poor, nearly duplicate, or tiring. The
acquisition objective is subordinate to validators, quality floors, diversity,
and a bounded question budget.

### Position and audition bias

Left/right order, first playback, volume, currently focused bar, and stale audio
state can dominate the judgement. The comparison protocol must capture order,
use the accepted playback stack, and counterbalance presentation deterministically.

### False certainty from correlated candidates

Variants from the same source, seed family, or strategy are not independent
evidence. Candidate lineage and equivalence policy must prevent a large clone
family from manufacturing confidence.

## 10. Non-goals

- Port CODA's Python implementation or confusion-matrix model.
- Add Python, PyTorch, MLflow, or a new runtime dependency to Griff.
- Train a neural reward model.
- Replace S9's simple explainable reranking baseline before measuring it.
- Treat favorite / reject counts as objective labels.
- Generate deliberately poor candidates to increase disagreement.
- Change frozen S6/S7 generation, existing rerank policy, or S8 playback.
- Assign a new stage number or claim that S9 is accepted.
- Turn a research posterior into a silent global user profile.

## 11. Roadmap relationship

This proposal is adjacent to S9 and depends on S8's captured candidate history,
but it does not amend the canonical S9 contract.

Recommended placement:

1. finish and accept S9 Phase 0 feedback capture semantics;
2. establish the existing simple S9 preference-reranking baseline;
3. run Active Preference Selection Phases 0–3 as an offline research increment;
4. decide from evidence whether the result is an S9 experiment, an ADR-backed
   infrastructure boundary, or no product work at all.

No stage number is invented here. A statistically elegant acquisition policy
without stable feedback provenance would be technical debt wearing a lab coat.

## 12. Open questions

- Is the first useful target a preferred **candidate**, **strategy**, or
  **configuration family**?
- Should persistent preference be learned globally, per request family, or only
  inside one session until enough evidence exists?
- Which unary events can be used without introducing fake pairwise outcomes?
- Is expected information gain or expected simple-regret reduction the better
  acquisition objective for the cockpit?
- What candidate equivalence rule prevents near-clones from dominating the
  posterior without erasing meaningful variations?
- How should favorite, approve, reject, reject-both, tie, and skip map to the
  simplest inspectable likelihood model?
- What complete-panel size remains musically useful without turning evaluation
  into unpaid quality assurance?
- Which context features are stable canonical facts rather than policy-derived
  measurements that will drift under future versions?

## 13. Decision requested

Approve only **Phase 0: audit and benchmark contract** as a future research task
that starts after the S9 feedback contract and simple baseline exist.

Do not approve an implementation, production dependency, cockpit behaviour
change, or roadmap reassignment from this proposal.
