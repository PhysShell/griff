# Constraint Inventory (hard-constraint-contract discussion)

Working research artifact for the hard-constraint-contract proposal: the
rules Griff already enforces (and where), the rules the research memos
proposed, and a classification of each under a closed taxonomy — so the
future `RuleScope`/`RuleEvaluation` layer formalizes reality instead of
inventing a parallel one, and so the MiniZinc oracle spike gets *real*
problems instead of a demo toy.

Status: for discussion (v1). Companion to
[`hard-constraint-contract.md`](hard-constraint-contract.md); that
proposal is the contract thesis, this file is the catalog.
Scope: docs-only; binds nothing.

**Acceptance gate (normative for this document).** The inventory
*classifies rules*; it promises no production semantics, no API, and no
delivery phase. Every `hard` classification still requires its own spec
section, failure-code vocabulary, characterization tests over today's
behaviour (SPEC hard rule 5 where behaviour already exists), and a
red→green slice. A classification here is a licence to *specify*, never a
decision already taken.

## Classification taxonomy (closed — exactly four states)

- **hard** — admissibility, evaluated in the rule layer
  (`RuleEvaluation`); any `Violated` rejects the candidate under the
  fail-closed aggregation policy of the contract proposal;
- **soft** — preference; lives in scoring (`Axes`/`WeightPolicy`,
  ADR-0017) and never enters the rule layer;
- **opt-in** — a scoped arrangement constraint the user explicitly turns
  on for a `RuleScope`; absent from the default rule set;
- **defer** — blocked on a named prerequisite; no classification decision
  exists until that prerequisite does.

Qualifiers live in the reason, never in the state. Rules that are hard
*within one subsystem's contract* but not universal are recorded as hard
with an explicit scope — scope is part of the rule, not a fifth state.

## Survey baseline — what is enforced today

- `core/src/fretboard.rs`: `infer_positions` (fingering DP, ADR-0019),
  `measure_playability` → `PlayabilityReport` — `is_playable()` is
  `unpositionable == 0`; `max_fret_jump` is a **carried fact, not a
  verdict**; chords are left unvoiced (deferred, ADR-0019 §7);
  `STANDARD_MAX_FRET = 24`.
- `core/src/complement.rs`: `PairValidation` — `coincident_dissonances`
  (m2 / tritone / M7 on coincident onsets), `register_mud`, per-part
  playability; `is_clean()` requires all four. Doc comment states the
  house position explicitly: "jump thresholds are calibration data, not
  code". `ComplementError` is a typed-refusal vocabulary
  (`InvalidSpec`, `NoGapsToAnswer`, `NonUniformTimeline`, …).
- `core/src/generate.rs`: `GenerationConstraints` — "hard constraints
  that all generated phrases must satisfy": `bar_count ≥ 1`, uniform
  time signature and tempo, `ticks_per_quarter > 0`,
  `pitch_lo ..= pitch_hi`.
- `core/src/tonal.rs` (S15 Phase 1): tonal estimates are **evidence
  only** — scoped, optional, and by the accepted S15 contract they
  restrict no pitch and change no production behaviour.
- `pattern/` + Swang seam: `ExpansionBudget` (typed refusal on breach),
  bar-geometry divisibility and tail policy (`SWG0301`/`SWG0302`), meter
  uniformity across a mapped span (`SWG0304`), `density` requires a seed
  (`SWG0303`).
- DP layer (ADR-0013/0030): transition and local costs are **soft** by
  construction; feasibility is expressed as graph shape, not as cost ∞.

## Summary

| # | Rule | Dimension | State | Enforced today |
| --- | --- | --- | --- | --- |
| 1 | fretboard reachability (line notes) | Fretboard | **hard** | yes — `is_playable`, per-part filter |
| 2 | chord voicing feasibility | Fretboard | **defer** | no — chords left unvoiced (ADR-0019 §7) |
| 3 | max fret travel | Fretboard | **soft** | fact carried, never a verdict |
| 4 | pitch range window | Pitch | **hard** | yes — `GenerationConstraints` |
| 5 | string conflict (simultaneous notes, one string) | Fretboard | **hard** (monophonic scope) | implicit via monophonic lines |
| 6 | coincident dissonance (pair) | Harmony × Complementarity | **hard** (complement scope) | yes — `PairValidation` |
| 7 | register mud (pair) | Pitch × Complementarity | **hard** (complement scope) | yes — `PairValidation` |
| 8 | strong-beat doubling (pair) | Complementarity | **opt-in** | no (decided in PR #149) |
| 9 | timeline uniformity preconditions | Structure | **hard** (per-mode scope) | yes — typed `ComplementError`s |
| 10 | bar-geometry / meter divisibility | Rhythm | **hard** | yes — `SWG0301/0302/0304` |
| 11 | seed presence for stochastic ops | Structure | **hard** | yes — `SWG0303`, `PruneSpec` |
| 12 | expansion / cycle budgets | Structure | **hard** | yes — `ExpansionBudget` |
| 13 | tapping-transition reachability | Technique × Fretboard | **defer** | no — needs the technique evidence model |
| 14 | harmonic-context membership | Harmony | **defer** | no — S15 forbids it by design today |
| 15 | voice crossing (named rule) | Pitch × Complementarity | **opt-in** | partially shadowed by register mud |

## Per-rule detail

Field key: *Scope* is stated in `RuleScope` terms (tracks / voices /
dimension / range / relation). *Lookback / lookahead* is what the rule
must see beyond the event it judges. *Evidence* is what a `Violated`
carries. *Incremental* — whether the rule can be re-evaluated per event
for DP clients (ADR-0013/0030) without whole-candidate recomputation.
*Oracle candidate* — suitability for the MiniZinc offline spike.

### 1. Fretboard reachability — hard

- **Statement**: every line note has at least one playable
  `(string, fret)` under the part's tuning within `max_fret`.
- **Scope**: per track/voice; dimension Fretboard; whole line; relation:
  per-note.
- **Today**: `PlayabilityReport.is_playable` (`unpositionable == 0`),
  used as the per-part filter (ADR-0019); typed loss report on import.
- **Lookback / lookahead**: none — reachability of a pitch is per-note
  (path *quality* is rule 3's soft territory).
- **Evidence**: the unpositionable note's musical path + pitch + tuning.
- **Incremental**: yes (per note).
- **Oracle candidate**: **yes — spike problem A.** The fingering DP is a
  heuristic over a real solution space; an exact model can (a) confirm
  UNSAT claims for unreachable lines and (b) hunt minimal
  counterexamples where the DP's greedy-ish path misses a feasible
  assignment under tighter travel bounds.

### 2. Chord voicing feasibility — defer

- **Statement**: simultaneous notes admit a non-conflicting joint
  `(string, fret)` assignment.
- **Prerequisite (named)**: the chord-voicing model deferred by
  ADR-0019 §7 — today chords are deliberately left unvoiced rather than
  misvoiced, and a rule cannot precede its model.
- **Note**: when it lands, this is the constraint-shaped problem par
  excellence (assignment + mutual exclusion) and an obvious later oracle
  target; it inherits rule 5 as a sub-constraint.

### 3. Max fret travel — soft

- **Statement**: consecutive-note fret jumps should stay small.
- **Today**: `max_fret_jump` is a carried fact; the complement doc
  comment is the standing decision — "jump thresholds are calibration
  data, not code".
- **Classification reason**: a threshold would be taste until calibrated
  against evidence (S9 territory); the fact is already stored, which is
  all the rule layer needs from it. Stays in scoring/weights.

### 4. Pitch range window — hard

- **Statement**: generated pitches lie in `pitch_lo ..= pitch_hi`.
- **Scope**: per generation request; dimension Pitch; per-note.
- **Today**: `GenerationConstraints` (documented as hard).
- **Lookback / lookahead**: none. **Incremental**: yes.
- **Evidence**: offending note + bounds.
- **Oracle candidate**: no — trivially checkable; needs no solver.

### 5. String conflict — hard (monophonic scope today)

- **Statement**: two simultaneously sounding notes never occupy one
  string.
- **Today**: implicit — the generator emits monophonic lines and chords
  are unvoiced, so the conflict cannot currently arise; nothing *checks*
  it.
- **Classification reason**: hard now with the honest scope note; the
  full polyphonic rule activates with rule 2's model and must land as an
  explicit check, not remain an accident of monophony.
- **Incremental**: yes (per onset group).

### 6. Coincident dissonance — hard (complement scope)

- **Statement**: no m2 / tritone / M7 between parts on coincident
  onsets, within the ComplementArranger's cleanliness contract.
- **Scope**: tracks A×B; dimension Harmony × Complementarity; relation:
  simultaneous onsets.
- **Today**: `PairValidation.coincident_dissonances`, part of
  `is_clean`.
- **Classification reason**: hard *within the complement contract* — as
  a universal rule it would outlaw intentional tension, so its scope is
  the contract, not the repertoire. Widening it beyond complement would
  be a new opt-in rule, not a widening of this one.
- **Lookback / lookahead**: the coincident onset pair only.
  **Incremental**: yes (per coincidence).
- **Evidence**: onset, both pitches, interval class.
- **Oracle candidate**: **yes — part of spike problem B** (below).

### 7. Register mud — hard (complement scope)

- **Statement**: parts A and B do not overlap registers so heavily that
  the pair loses separation.
- **Today**: `PairValidation.register_mud`, part of `is_clean`.
- **Note**: the boolean compresses a graded fact; when the rule layer
  formalizes it, the underlying overlap measurement should surface as
  evidence (same raw-fact-vs-verdict discipline as rule 3).
- **Incremental**: windowed (needs both parts' registers over a span).
- **Oracle candidate**: yes — as a register-window constraint in spike
  problem B.

### 8. Strong-beat doubling — opt-in

- **Statement**: the complement does not double the lead on strong
  beats.
- **State source**: decided in the accepted PR #149 revision — a
  strong-beat unison is a legitimate arrangement device; the rule enters
  as an explicitly scoped opt-in, soft penalty by default.
- **Today**: not implemented. **Incremental**: yes (per strong beat).

### 9. Timeline uniformity preconditions — hard (per-mode scope)

- **Statement**: certain arrangement modes require a uniform meter/span
  across the arranged range (e.g. `counter_melody`).
- **Today**: typed refusals — `ComplementError::NonUniformTimeline`,
  `InvalidSpec`, `NoGapsToAnswer` — already model the contract
  proposal's `Unsupported`/`Violated` split in miniature.
- **Classification reason**: these are problem-level preconditions, the
  easiest rules to port into `RuleEvaluation` because their vocabulary
  is already typed.

### 10. Bar-geometry / meter divisibility — hard

- **Statement**: a declared unit divides the bar exactly and is
  tick-representable (`SWG0301`); incomplete tails follow the declared
  tail policy (`SWG0302`); a mapped span keeps one meter (`SWG0304`).
- **Today**: typed errors in the Swang seam; the master timeline is the
  single source of truth (SPEC hard rule 3).
- **Incremental**: per span; cheap.

### 11. Seed presence — hard

- **Statement**: every stochastic operation names its seed; `density`
  without `seed` is refused (`SWG0303`); `PruneSpec` carries the rhythm
  seed by law.
- **Classification reason**: determinism (SPEC hard rule 6) expressed as
  an admissibility rule on *requests*, not on musical content — the rule
  layer should keep that distinction visible.

### 12. Expansion / cycle budgets — hard

- **Statement**: expansion never exceeds its declared budget; breach is
  a typed refusal, never truncation.
- **Today**: `ExpansionBudget` in `griff-pattern`; the operator
  inventory adds `CycleBudget` to the same family (Kani harnesses for
  the budget invariant are already a process-backlog item).

### 13. Tapping-transition reachability — defer

- **Statement**: technique transitions (tapping entries/exits) are
  physically reachable in time.
- **Prerequisite (named)**: a technique evidence model over the rich
  note model (ADR-0018) that defines *what* a transition is and *when*
  reachability is measurable. Classifying it hard before that model
  exists would invent the model by side effect (the reset-on-event
  lesson from the operator inventory).

### 14. Harmonic-context membership — defer

- **Statement**: notes lie within a scoped harmonic context.
- **State source**: the accepted S15 contract — context is optional
  evidence with **no pitch restriction and no production behaviour
  change**; chromatic passing tones, borrowed notes and tensions stay
  legal by design (recorded in the contract proposal after the PR #149
  Codex finding).
- **Prerequisite (named)**: a later, explicitly calibrated contract on
  top of the accepted S15 phases. Until then tonal context influences
  nothing in the rule layer.

### 15. Voice crossing — opt-in

- **Statement**: part B does not cross above/below part A where the
  arrangement declares a register order.
- **Today**: not implemented as a named rule; heavy overlap partially
  shadowed by register mud (rule 7), which is a different fact
  (co-occupancy, not order inversion).
- **Classification reason**: like rule 8 — a legitimate device in some
  arrangements (Cluster Engine treats it as a *selectable* rule, which
  is the right prior art posture), so opt-in per `RuleScope`, never
  universal.

## Oracle spike selection (the two real problems)

Per the Constraint Lab first increment, the spike models **existing**
rules, not invented toys:

- **Problem A — fretboard realization** (rule 1, with rule 3 as data):
  given a monophonic line, tuning, `max_fret`, and a travel bound,
  decide satisfiability of a `(string, fret)` assignment and return a
  witness or UNSAT. Compared against `infer_positions` on the same
  inputs: confirms unreachability claims exactly and hunts minimal
  counterexamples where the DP misses feasible assignments under tight
  bounds. Fixture export: deterministic lines + tunings, archived with
  solver identity.
- **Problem B — complementary pair cleanliness** (rules 6, 7, and
  opt-in 15): given part A fixed and a candidate rhythm for part B,
  decide whether any pitch assignment for B satisfies no-coincident-
  dissonance + register windows (+ optional no-crossing), or prove the
  window empty. This checks `PairValidation`'s rule set for emptiness —
  exactly the "prove UNSAT honestly" use the Lab exists for.

Both problems archive exact inputs/outputs per the Lab's manifest
discipline; neither touches production (`deny.toml` unchanged).

## What the inventory deliberately does not contain

Workspace-level laws (forbid-unsafe, lint gates, TDD, English-only) —
they are repository constitution (SPEC/AGENTS), not candidate-admissibility
rules, and putting them in a `RuleScope` would confuse two different kinds
of "hard". Scoring axes (`rerank.rs`, relation weights) — soft by
definition, catalogued where they live. Rules whose subject matter is
deferred by other documents (learned-model authority boundaries — the
preference-learning proposal's non-goals govern).
