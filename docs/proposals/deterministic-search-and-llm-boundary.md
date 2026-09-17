# Proposal: Deterministic search, diversity, and the LLM boundary

Status: proposal for discussion
Scope: docs-only; binds nothing until accepted

## 1. Goal

Make Griff less dependent on opaque model behaviour without reducing the
musical search space to one sterile optimum.

The proposed boundary is:

```text
optional LLM or human authoring
        |
        v
ArrangementIntent (typed, versioned, validated)
        |
        v
candidate construction + hard-rule admission
        |
        v
exact or explicitly approximate search
        |
        v
ranked, explainable, reproducible alternatives
```

The LLM may describe an arrangement role and preferences. It does not emit the
canonical score, assign strings and frets, bypass hard rules, or decide that its
own result is valid.

This proposal extends the direction already fixed by ADR-0013 and ADR-0030:
DP/Viterbi remains the primary route over an enumerated layered graph. It does
not replace the existing Constraint Lab, hard-constraint proposal, scoring
contract, or deterministic generators.

## 2. Existing ground truth

This proposal starts from what Griff already has, rather than presenting old
work in a new hat.

- `core/src/layered_path.rs` provides domain-free layered DP with deterministic
  tie-breaking and a normative floating-point association (ADR-0030).
- ADR-0017 defines axes, versioned weights, rationale, derived aggregate, and
  provenance; hard gates remain separate from soft ranking.
- `docs/proposals/hard-constraint-contract.md` defines typed admissibility and
  keeps external solvers outside the production path.
- `lab/` already implements the first MiniZinc oracle spike with a solver-neutral
  IR, an exact reference solver, archived solver identities, and five agreeing
  fixtures. The next search work must consume that evidence instead of proposing
  the same spike again.

## 3. Decision under discussion

### 3.1 Keep one-best DP as the normative baseline

For an already enumerated layered graph, exact DP/Viterbi remains the default.
It visits the supplied transitions directly, requires no heuristic, and returns
one globally optimal path under the selected `WeightPolicy`.

The one-best result is the baseline against which every later search mode is
characterized. A new mode may produce more alternatives or trade optimality for
latency, but it may not silently change what `best` means.

### 3.2 Add deterministic k-best traversal before A*

The next search capability should return the first `k` distinct complete paths
in total order:

```text
primary:   derived path cost
secondary: canonical path identity
tertiary:  per-layer candidate ordinal sequence
```

The exact algorithm is deferred to an ADR and benchmark. Candidate families
include k-best Viterbi, lazy k-shortest-path enumeration, and deviation-based
methods. The implementation must reuse the same local and transition-cost
contract as one-best DP; a second cost implementation is forbidden.

Required properties:

- `k = 1` is byte-for-byte equivalent to the normative one-best result;
- output order is total and stable;
- duplicate canonical paths are impossible;
- each path carries the same `Scored` axes, rationale, weights version, and
  generation provenance as an ordinary result;
- requesting more results never changes the prefix already returned;
- finite budgets yield a typed incomplete result, never a falsely complete set.

This creates reproducible alternatives without treating repeated RNG runs as a
search algorithm.

### 3.3 Diversity is a policy over valid alternatives

`DiversityPolicy` is separate from score weights and hard rules. It selects a
useful subset from an ordered valid candidate stream; it does not redefine
legality and does not mutate the underlying score.

Candidate measurable terms:

```text
minimum canonical path distance
maximum shared-candidate ratio
maximum identical-technique run
motif reuse budget
rhythm-cell distance
register-region distance
fretboard-route distance
```

Every term must name a deterministic measurement over canonical material.
Words such as `fresh`, `creative`, and `swancore enough` are presets or human
judgements, not diversity metrics.

Two modes are worth distinguishing:

1. **Filter mode** — walk k-best order and admit the next path satisfying the
   policy relative to those already selected.
2. **Penalty mode** — apply a versioned diversity objective during subsequent
   enumeration.

Filter mode should be attempted first because it preserves the original score
ordering and keeps diversity inspectable. Penalty mode requires a separate ADR
because it changes the optimization problem.

A diversity result records:

```text
source search contract + version
DiversityPolicy identity + version
selected path identities
pairwise measurements used for admission/refusal
search budget and completeness status
```

### 3.4 Add lexicographic and Pareto views without weakening gates

A weighted aggregate is useful for total ordering, but the axes remain the
truth. Griff should support analysis views that expose:

- lexicographic ordering over named axes;
- Pareto-nondominated candidate sets;
- weighted ordering within a chosen admissible set.

These are ranking views only. The pipeline remains:

```text
hard-rule admission
        -> optional lexicographic/Pareto selection
        -> weighted total order where required
```

No Pareto or weighted comparison may revive a candidate rejected by a hard
rule. A beautiful unplayable part remains unplayable, however moving its
rationale may be.

### 3.5 Define `ArrangementIntent` as the only LLM-facing generation contract

An LLM integration, if added, produces a typed intent rather than notes or a
patch to the canonical score.

Illustrative shape:

```text
ArrangementIntentV1
  reference material identity
  requested section/range
  relationship mode
  section roles
  rhythm strategy
  harmonic strategy
  register policy
  technique budgets
  energy curve
  enabled opt-in rules
  soft-preference preset / weight-policy reference
```

Contract rules:

- all referenced score objects and ranges must resolve before execution;
- enums and numeric ranges are closed and versioned;
- intent canonicalization occurs before hashing or execution;
- unsupported or contradictory fields produce typed refusals;
- the LLM cannot provide executable code, arbitrary rule expressions, direct
  file paths, raw MIDI events, or unrestricted scoring formulas;
- generated score material is produced only by deterministic Griff components;
- every result binds the exact intent digest, model/provider receipt where an
  LLM was involved, generator version, rule set, weights version, and seed where
  a stochastic component is explicitly part of the contract.

A human-authored intent and an LLM-authored intent are the same object after
validation. No downstream component branches on whether prose or silicon
produced it.

### 3.6 Treat an LLM as an optional proposer, never an authority

The minimum integration protocol is:

```text
untrusted model response
  -> syntax validation
  -> structural validation
  -> repository/score-reference validation
  -> policy validation
  -> canonical ArrangementIntent
  -> deterministic execution
  -> hard-rule admission
```

Provider-side structured output is an ergonomic aid, not the trust boundary.
The local validator remains authoritative because closed providers expose
different schema subsets and can still return semantically invalid but
well-formed values.

The production core must remain usable with no LLM configured. Closed and open
models are adapters above the same intent contract; no safety or validity
property may depend on access to logits, weights, or a provider seed.

## 4. When A* becomes justified

A* is not the next replacement for layered DP. It becomes a candidate only for
a client whose state graph is implicit and generated lazily, for example a
partial phrase with unresolved rhythmic obligations, motif callbacks,
fretboard state, and a target closure condition.

An A* proposal must name all of the following:

```text
state identity and canonicalization
actions and deterministic successor order
goal predicate
path-cost contract
heuristic definition
proof or test of admissibility/consistency where exact optimality is claimed
budget and incomplete-search result
provenance for approximate modes
```

Acceptance requires a representative benchmark against the simplest applicable
baseline: exact layered DP when the graph can be enumerated, otherwise Dijkstra
or uniform-cost search over the same implicit client.

Kill criteria:

- no material reduction in expanded states or latency;
- heuristic computation consumes the saved work;
- state canonicalization cannot prevent duplicate exploration;
- result quality depends on undocumented successor order;
- the client can be represented naturally as a manageable layered graph;
- an approximate mode cannot clearly report its suboptimality contract.

Weighted A* may later serve an interactive preview, but it must use a distinct
search-mode identity. `best` may not quietly become `first result before the UI
became impatient`.

## 5. Constraint Lab role

The Constraint Lab remains an offline oracle, never a production dependency.
Its next useful targets are:

- chord-voicing feasibility once the deferred model exists;
- bounded small instances used to verify k-best uniqueness and ordering;
- counterexample generation for new hard-rule interactions;
- exact feasibility witnesses for implicit-search clients;
- differential checks between production search and solver-neutral models.

The Lab is not a shadow runtime. Production results may be compared with pinned
oracle runs in tests and audits, but preview and generation cannot shell out to
MiniZinc or a CP solver.

Every oracle extension preserves the current discipline: exact input, frontend
and backend identities, archived outcome, independently checked SAT witnesses,
and solver failure distinct from UNSAT.

## 6. Determinism and provenance contract

Every search result must be reproducible relative to a complete contract, not a
seed alone.

Minimum provenance:

```text
canonical input identities and digests
search mode and version
state/client version
hard-rule set and version
weight policy and version
diversity policy and version, if any
normative arithmetic contract
canonical tie-break contract
budget and completeness status
seed only for explicitly stochastic upstream construction
```

Changing any item creates a different result contract. Stored paths without
this provenance are display artifacts, not reproducible search evidence.

## 7. Proposed delivery order

This proposal does not assign roadmap stages. If accepted, implementation work
should be split into independently reviewable ADRs/slices:

1. **Status reconciliation** — confirm whether ADR-0017 is still truthfully
   `Proposed` now that `Scored` has multiple code consumers; update canonical
   status only through the repository's normal review process.
2. **k-best contract and characterization** — specify total ordering,
   canonical path identity, completeness, and `k = 1` equivalence before code.
3. **k-best implementation** — tests first, same cost evaluator, bounded API.
4. **DiversityPolicy v1** — deterministic filter mode over k-best results with
   measurable path-distance evidence.
5. **ArrangementIntent v1** — schema, canonicalization, typed refusals, and a
   human-authored adapter before any LLM provider adapter.
6. **LLM adapter spike** — one optional provider behind the local validator;
   no production authority and no core dependency.
7. **A* client spike, only if earned** — a named implicit client plus benchmark
   and kill criteria.

## 8. Validation expectations

Documentation acceptance should require consistency with ADR-0013, ADR-0030,
ADR-0017, the hard-constraint proposal, the Constraint Inventory, and the
existing Lab report.

Later implementation acceptance should include:

- exhaustive tiny layered graphs compared with brute-force enumeration;
- metamorphic tests for stable prefixes as `k` grows;
- duplicate-state and duplicate-path adversarial fixtures;
- deterministic tie fixtures including equal aggregate costs;
- arithmetic edge cases and non-finite accumulation refusals;
- diversity fixtures proving every selected/refused pair measurement;
- intent canonicalization and hash stability tests;
- invalid, contradictory, stale-reference, and unsupported intent fixtures;
- differential oracle cases where the Lab has a matching bounded model;
- performance thresholds stated before selecting a more complex algorithm.

## 9. Non-goals

- Replacing DP/Viterbi with A* globally.
- Making an LLM mandatory for generation.
- Letting an LLM write canonical notes, tabs, rules, or score weights directly.
- Treating random seeds as a substitute for explicit diversity.
- Adding MiniZinc, CP-SAT, or another solver to the production path.
- Collapsing hard constraints, diversity, and musical preferences into one
  scalar.
- Claiming that k-best paths are musically diverse without a measured policy.
- Training or fine-tuning a model in this proposal.

## 10. Prior art to evaluate in the accepting ADRs

The implementation ADRs should survey, benchmark, and cite the original or
primary descriptions of:

- Viterbi and k-best dynamic-programming variants;
- Yen-style deviation enumeration for loopless k-shortest paths;
- Eppstein-style k-shortest-path enumeration where its graph assumptions fit;
- diverse M-best / diversity-aware structured prediction;
- A* and weighted A* for implicit state spaces;
- MiniZinc solution enumeration and diversity mechanisms;
- CP/SAT no-good constraints for enumerating distinct feasible solutions.

Idea reuse is expected; code reuse remains subject to Griff's licence and
dependency policy.
