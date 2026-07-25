# Constraint Inventory (hard-constraint-contract discussion)

Working research artifact for the hard-constraint-contract proposal: the
rules Griff already enforces (and *how* — wired gate vs validator vs
by-construction), the rules the research memos proposed, and a
classification of each under a closed taxonomy — so the future
`RuleScope`/`RuleEvaluation` layer formalizes reality instead of
inventing a parallel one, and so the MiniZinc oracle spike gets real,
production-grounded problems.

Status: for discussion (v2 — v1 revised per the arbiter review of
PR #151: rule subjects, honest enforcement status, refusal-class
mapping, exact Problem B contract).
Companion to [`hard-constraint-contract.md`](hard-constraint-contract.md);
that proposal is the contract thesis, this file is the catalog.
Scope: docs-only; binds nothing.

**Acceptance gate (normative for this document).** The inventory
*classifies rules*; it promises no production semantics, no API, and no
delivery phase. Every `hard` classification still requires its own spec
section, failure-code vocabulary, characterization tests over today's
behaviour (SPEC hard rule 5 where behaviour already exists), and a
red→green slice. A classification here is a licence to *specify*, never
a decision already taken.

## Classification taxonomy (closed — exactly four states)

- **hard** — admissibility; any `Violated` rejects under the fail-closed
  aggregation policy of the contract proposal;
- **soft** — preference; lives in scoring (`Axes`/`WeightPolicy`,
  ADR-0017) and never enters the rule layer;
- **opt-in** — a scoped arrangement constraint the user explicitly turns
  on for a `RuleScope`; absent from the default rule set;
- **defer** — blocked on a named prerequisite; no classification
  decision exists until that prerequisite does.

Qualifiers live in the reason, never in the state.

### Rule subject (orthogonal to the state, not a fifth state)

"Hard" alone conflates four incompatible things. Every rule also
declares **what it judges**:

- **Candidate** — produced musical content; the only subject
  `RuleEvaluation` over a candidate ever sees;
- **Request** — a generation/arrangement request, mode, or program,
  judged before any candidate exists;
- **Execution** — the computation itself (budgets, refusal instead of
  truncation).

The existing `RuleScope` (tracks / voices / dimension / range /
relation) describes Candidate rules only; Request and Execution rules
need their own, smaller scope vocabulary in the contract. Without this
split a future `RuleEvaluation` either bloats into checking everything,
or pretends a missing seed is a property of a generated note.

### Enforcement status (closed vocabulary)

"Enforced today" overstated v1. The honest states:

- **wired gate** — production path actually refuses on violation;
- **validator only** — a checker exists and is exercised (tests/fuzz),
  but no production path calls it as an admission gate;
- **by construction** — the property cannot be violated because the
  producing code only emits conforming output (this is a fact about
  today's producer, not a check);
- **vacuous** — the violating situation cannot currently arise, and
  nothing checks it;
- **not implemented**.

## Survey baseline — what exists today

- `core/src/fretboard.rs`: `infer_positions` (exhaustive weighted-cost
  fingering DP, ADR-0019), `measure_playability` → `PlayabilityReport`;
  `is_playable()` is `unpositionable == 0`; `max_fret_jump` is a
  **carried fact, not a verdict**; chords are left unvoiced (deferred,
  ADR-0019 §7); `STANDARD_MAX_FRET = 24`.
- `core/src/complement.rs`: `validate_pair` → `PairValidation`
  (`coincident_dissonances` over `DISSONANT_CLASSES = [1, 6, 11]`
  mod 12 on coincident onsets; `register_mud` =
  `band_overlap(bandA, bandB) > 0.5` over whole-part pitch bands,
  overlap measured relative to the **narrower** band, degenerate
  single-pitch band overlapping iff the point lies inside; per-part
  playability on the highest-pitch-per-onset line under the part's own
  tuning, `FingeringWeights::v1`, standard fret range — a chord
  participates through its top note only). `is_clean()` requires all
  four. **`validate_pair` is a validator, not a wired gate**: the
  arranger produces and returns candidates without calling it; its
  callers today are tests and the `complement_request` fuzz target.
  `ComplementError` is a typed refusal vocabulary on *requests*.
- `core/src/generate.rs`: `GenerationConstraints` — bar count, uniform
  meter/tempo, tick resolution, pitch window; the window is
  **normalized** via `PitchRange::new` (bounds accepted in either
  order, clamped to valid MIDI) and the generator draws inside it by
  construction.
- `core/src/tonal.rs` (S15 Phase 1, accepted): tonal estimates are
  **evidence only** — scoped, optional, restricting no pitch. The
  generation-facing scoped-context contract with its explicit
  no-pitch-restriction / no-production-change acceptance gate is S15
  **Phase 2 — proposed, not yet accepted**; today's guarantee rests on
  the accepted Phase-1 evidence-only behaviour.
- `pattern/` + Swang seam: `ExpansionBudget` (typed refusal on breach),
  bar-geometry divisibility and tail policy (`SWG0301`/`SWG0302`),
  meter uniformity across a mapped span (`SWG0304`), `density` requires
  a seed (`SWG0303`).
- DP layer (ADR-0013/0030): transition and local costs are **soft** by
  construction; feasibility is expressed as graph shape.

## Summary

| # | Rule | Subject | Dimension | State | Current status |
| --- | --- | --- | --- | --- | --- |
| 1 | fretboard reachability (line notes) | Candidate | Fretboard | **hard** | validator only (+ import loss report) |
| 2 | chord voicing feasibility (incl. unvoiced-chord string assignment) | Candidate | Fretboard | **defer** | not implemented (ADR-0019 §7) |
| 3 | max fret travel | Candidate | Fretboard | **soft** | fact carried, never a verdict |
| 4 | pitch window (normalized) | Candidate | Pitch | **hard** | by construction (generator domain) |
| 5 | known-position string exclusivity | Candidate | Fretboard | **hard** | not implemented (vacuous for generated lines) |
| 6 | coincident dissonance (pair) | Candidate | Harmony × Complementarity | **hard** (complement scope) | validator only |
| 7 | register mud (pair) | Candidate | Pitch × Complementarity | **hard** (complement scope) | validator only |
| 8 | forbid strong-beat doubling (pair) | Candidate | Complementarity | **opt-in** | not implemented |
| 9 | complement request/mode refusals | Request | Structure | **hard** | wired gate (mapping below) |
| 10 | bar-geometry / meter divisibility | Request | Rhythm | **hard** | wired gate (`SWG0301/0302/0304`) |
| 11 | seed presence for stochastic ops | Request | Structure | **hard** | wired gate (`SWG0303`, `PruneSpec`) |
| 12 | expansion / cycle budgets | Execution | Structure | **hard** | wired gate (`ExpansionBudget`) |
| 13 | tapping-transition reachability | Candidate | Technique × Fretboard | **defer** | not implemented |
| 14 | harmonic-context membership | Candidate | Harmony | **defer** | not implemented (S15 forbids it today) |
| 15 | voice crossing (named rule) | Candidate | Pitch × Complementarity | **opt-in** | not implemented |

## Per-rule detail

Field discipline (per the contract proposal): every rule carries Scope,
Current status, Lookback/lookahead, Evidence, Incremental, Failure code,
Oracle candidacy — with an explicit `N/A` or `TBD at spec` where a field
is consciously not applicable yet, never a silent omission.

### 1. Fretboard reachability — hard · Candidate

- **Statement**: every line note has at least one playable
  `(string, fret)` under the part's tuning within `max_fret`.
- **Scope**: per track/voice; dimension Fretboard; whole line; per-note
  relation.
- **Current status**: **validator only** — `is_playable` exists and is
  exercised (complement validator, tests, fuzz); the MIDI import path
  *reports* unpositionable notes as losses (ADR-0019 §1) rather than
  refusing. No production path today rejects a candidate for
  unreachability; wiring it as a gate is exactly what the rule layer
  would add.
- **Lookback / lookahead**: none — reachability is per-note existence
  (path *quality* is rule 3's soft territory).
- **Evidence**: unpositionable note's musical path + pitch + tuning.
- **Failure code**: TBD at spec.
- **Incremental**: yes (per note).
- **Oracle candidate**: only via the experimental Problem A framing
  below — plain reachability needs no solver, and `infer_positions`
  exhaustively minimizes its weighted cost with travel deliberately
  soft, so no bounded model can second-guess it.

### 2. Chord voicing feasibility — defer · Candidate

- **Statement**: simultaneous pitches admit a joint, non-conflicting
  `(string, fret)` assignment (subsumes string exclusivity for the
  unvoiced case).
- **Prerequisite (named)**: the chord-voicing model deferred by
  ADR-0019 §7 — today chords are deliberately left unvoiced rather than
  misvoiced, and a rule cannot precede its model.
- **Scope / Evidence / Failure code**: TBD with the model.
  **Lookback / Incremental**: N/A until specified.
- **Oracle candidate**: yes, *later* — assignment + mutual exclusion is
  the constraint-shaped problem par excellence, but only once the model
  exists to ground it.

### 3. Max fret travel — soft · Candidate

- **Statement**: consecutive-note fret jumps should stay small.
- **Scope**: per track/voice; dimension Fretboard; relation: consecutive
  positioned line notes.
- **Current status**: `max_fret_jump` is a carried fact; the standing
  decision is in the code — "jump thresholds are calibration data, not
  code".
- **Classification reason**: a threshold would be taste until calibrated
  against evidence (S9 territory); the fact is already stored, which is
  all scoring needs.
- **Lookback / lookahead**: the previous positioned note only.
- **Evidence / failure code**: N/A (soft — never a verdict).
- **Incremental**: yes (per adjacent positioned pair).
- **Oracle relation**: Problem A produces exactly the calibration
  evidence a future promotion decision would need (see below).

### 4. Pitch window — hard · Candidate

- **Statement**: generated pitches lie within the **normalized** window
  of the two request bounds. Normalization comes first and is part of
  the rule: production passes both fields through `PitchRange::new`,
  which accepts bounds in either order (swapping `pitch_lo > pitch_hi`)
  and clamps to valid MIDI — a reversed request is *supported*, not
  rejected, and a regression test pins that. Formalizing the literal
  `pitch_lo ..= pitch_hi` reading would misclassify inputs production
  accepts today.
- **Scope**: per generation request; dimension Pitch; per-note.
- **Current status**: **by construction** — the generator draws from a
  ladder inside the normalized window; nothing re-checks emitted notes.
  The rule layer would add the (cheap) explicit check.
- **Lookback / lookahead**: none. **Incremental**: yes.
- **Evidence**: offending note + normalized bounds.
  **Failure code**: TBD at spec.
- **Oracle candidate**: no — trivially checkable.

### 5. Known-position string exclusivity — hard · Candidate

- **Statement**: simultaneously sounding notes **with explicit
  positions** never occupy one string. Where positions are absent
  (unvoiced MIDI chords) the rule evaluates `Unknown`/`Unsupported` —
  it does not guess; feasibility of assigning positions is rule 2's
  deferred territory.
- **Why the v1 framing was wrong**: v1 called this "hard (monophonic
  scope)" because generated lines are monophonic — but that is a
  vacuously true statement in a scope where the conflict cannot arise,
  not a rule. And Griff is not only generated MIDI: Guitar Pro import
  preserves **explicit** fretboard positions, where the check is
  meaningful today.
- **Scope**: per track/voice; dimension Fretboard; relation:
  simultaneous onsets with known positions.
- **Current status**: **not implemented** (and vacuous for generated
  monophonic lines); nothing checks GP-imported explicit positions
  either.
- **Lookback / lookahead**: the onset group only. **Incremental**: yes.
- **Evidence**: onset, the two notes, the shared string.
  **Failure code**: TBD at spec.
- **Oracle candidate**: no on its own; folded into rule 2's later
  problem.

### 6. Coincident dissonance — hard (complement scope) · Candidate

- **Statement**: no interval in classes `{1, 6, 11}` mod 12 (m2 /
  tritone / M7) between parts on coincident onsets, within the
  ComplementArranger's cleanliness contract.
- **Scope**: tracks A×B; dimension Harmony × Complementarity; relation:
  simultaneous onsets.
- **Current status**: **validator only** — counted by `validate_pair`;
  the arranger does not call it before returning a candidate. "Clean
  pair" is today a *measurable* property, not a *guaranteed* one.
- **Classification reason**: hard *within the complement contract* — as
  a universal rule it would outlaw intentional tension. Widening beyond
  complement would be a new opt-in rule, not a widening of this one.
- **Lookback / lookahead**: the coincident onset pair.
  **Incremental**: yes (per coincidence).
- **Evidence**: onset, both pitches, interval class.
  **Failure code**: TBD at spec.
- **Oracle candidate**: yes — Problem B.

### 7. Register mud — hard (complement scope) · Candidate

- **Statement**: parts A and B do not overlap registers so heavily that
  the pair loses separation. The current law is exact and whole-part:
  `band_overlap(bandA, bandB) > 0.5`, where bands are each part's
  global `(lowest, highest)` pitch, overlap is measured relative to the
  **narrower** band, and a degenerate single-pitch band overlaps iff
  the point lies inside the other band.
- **Current status**: **validator only** (`validate_pair`), same as
  rule 6.
- **Note**: the boolean compresses a graded fact; the rule layer should
  surface the overlap fraction as evidence (raw-fact-vs-verdict
  discipline, as with rule 3).
- **Lookback / lookahead**: whole part. **Incremental**: extrema are
  maintainable per event, but the verdict is a whole-part decision over
  the final bands — no early final answer exists mid-candidate.
- **Evidence**: both bands + overlap fraction + threshold.
  **Failure code**: TBD at spec.
- **Oracle candidate**: yes — Problem B (as the exact band law, not a
  vague "register window").

### 8. Forbid strong-beat doubling — opt-in · Candidate

- **Statement**: the complement does not double the lead on strong
  beats. Two distinct named objects, one state each (the closed
  taxonomy permits no hybrid row):
  - `forbid_strong_beat_doubling` — the **opt-in** rule catalogued
    here;
  - `strong_beat_doubling_penalty` — its **soft** scoring counterpart,
    living with the other axes, not in this inventory's rule layer.
- **State source**: the PR #149 decision — a strong-beat unison is a
  legitimate arrangement device.
- **Scope**: tracks A×B; strong beats per the master timeline.
- **Current status**: not implemented (either object).
- **Lookback / lookahead**: the strong-beat onset pair.
  **Incremental**: yes. **Evidence**: beat position + doubled pitch.
  **Failure code**: TBD at spec. **Oracle candidate**: as an optional
  extension of Problem B only.

### 9. Complement request/mode refusals — hard · Request

- **Statement**: an arrangement request must be compatible with its
  mode's contract. The existing `ComplementError` vocabulary is typed,
  **and its variants mean three different things** — the mapping the
  rule layer must preserve rather than flatten:

| Variant | Meaning | Contract-proposal analogue |
| --- | --- | --- |
| `InvalidSpec` | ill-formed / incompatible request | request invalidity (`Violated` on a Request subject) |
| `NonUniformTimeline` | the mode cannot operate on this timeline | `Unsupported` |
| `NoGapsToAnswer` | the mode ran and found no admissible answer | a **no-solution outcome** — the production analogue of an empty search space, *not* a candidate violation (no candidate ever existed) |

- **Scope**: per arrangement request (mode + timeline + track
  selection) — a Request subject, outside `RuleScope`'s candidate
  vocabulary.
- **Current status**: **wired gate** — `arrange_complement` actually
  refuses with these errors.
- **Classification reason**: v1 claimed the "typed split already
  exists"; more precisely, the typed *vocabulary* exists, and the
  semantic split above is what porting must make explicit.
- **Lookback / Incremental**: N/A (request-level).
  **Evidence**: the variant itself + request. **Failure code**: exists
  (the variants).

### 10. Bar-geometry / meter divisibility — hard · Request

- **Statement**: a declared unit divides the bar exactly and is
  tick-representable (`SWG0301`); incomplete tails follow the declared
  tail policy (`SWG0302`); a mapped span keeps one meter (`SWG0304`).
- **Scope**: per mapped span of a request (declared unit + the master
  timeline's bar geometry) — a Request subject.
- **Current status**: **wired gate** (typed errors in the Swang seam);
  master timeline is the single source of truth (SPEC hard rule 3).
- **Lookback / lookahead**: the mapped span. **Incremental**: per span;
  cheap. **Evidence / failure codes**: exist (`SWG03xx`).

### 11. Seed presence — hard · Request

- **Statement**: every stochastic operation names its seed; `density`
  without `seed` is refused (`SWG0303`); `PruneSpec` carries the rhythm
  seed by law.
- **Classification reason**: determinism (SPEC hard rule 6) expressed
  as admissibility of *requests*, never of musical content — the
  subject split exists precisely to keep this visible.
- **Scope**: per request/program (every stochastic operation's
  parameter set) — a Request subject.
- **Current status**: **wired gate**. **Evidence / failure code**:
  exist (`SWG0303`; `PruneSpec` requires the field by type).
  **Lookback / Incremental**: N/A (request-level).

### 12. Expansion / cycle budgets — hard · Execution

- **Statement**: expansion never exceeds its declared budget; breach is
  a typed refusal, never truncation.
- **Current status**: **wired gate** — `ExpansionBudget` in
  `griff-pattern`; the operator inventory adds `CycleBudget` to the
  same family (Kani harnesses for the budget invariant are a
  process-backlog item).
- **Scope**: the computation, not the music — which is exactly why
  Execution is its own subject. **Evidence**: budget + attempted size.
  **Lookback / Incremental**: N/A.

### 13. Tapping-transition reachability — defer · Candidate

- **Statement**: technique transitions (tapping entries/exits) are
  physically reachable in time.
- **Prerequisite (named)**: a technique evidence model over the rich
  note model (ADR-0018) defining *what* a transition is and *when*
  reachability is measurable. Classifying it hard before that model
  exists would invent the model by side effect.
- **Other fields**: TBD with the model / N/A.

### 14. Harmonic-context membership — defer · Candidate

- **Statement**: notes lie within a scoped harmonic context.
- **State source**: the S15 stage plan — accepted Phase 1 makes tonal
  context evidence-only, and the *proposed* Phase 2 contract carries an
  explicit acceptance gate of **no pitch restriction and no production
  behaviour change**; chromatic passing tones, borrowed notes and
  tensions stay legal by design (recorded in the contract proposal
  after the PR #149 Codex finding). Nothing in the accepted or proposed
  S15 phases permits a context-based hard rule.
- **Prerequisite (named)**: a later, explicitly calibrated contract on
  top of the S15 phases *as they are accepted*.
- **Other fields**: TBD with that contract / N/A.

### 15. Voice crossing — opt-in · Candidate

- **Statement**: part B does not cross above/below part A where the
  arrangement declares a register order — a different fact from rule
  7's co-occupancy (mud measures overlap, crossing measures order
  inversion).
- **Classification reason**: a legitimate device in some arrangements;
  Cluster Engine treats it as a *selectable* rule, which is the right
  prior-art posture. Opt-in per `RuleScope`, never universal.
- **Current status**: not implemented as a named rule.
- **Lookback / lookahead**: coincident or windowed onsets.
  **Incremental**: yes. **Evidence**: onset span + the inverted pair.
  **Failure code**: TBD at spec.
- **Oracle candidate**: optional named extension of Problem B — not
  part of `PairValidation` v1.

## Oracle spike selection

The spike models **one existing rule set and one explicitly
experimental, production-grounded counterfactual**. Experimental
results provide calibration evidence and have **no authority over
production**.

- **Problem A — bounded-travel fretboard realization (experimental
  rule)**: given a monophonic line, tuning, `max_fret`, and an explicit
  travel bound, decide satisfiability of a `(string, fret)` assignment
  and return a witness or UNSAT. Two things this deliberately is *not*:
  it is not the existing reachability rule (rule 1 is per-note
  existence and needs no solver), and it is not a check on
  `infer_positions` — the production DP exhaustively minimizes a
  weighted cost in which travel is soft by standing decision (rule 3),
  so a bounded model answers a different question and a bounded UNSAT
  is never a "missed feasible assignment". What the spike measures:
  where an *experimental* hard travel bound would bite — which real
  lines become UNSAT at which bounds — producing calibration evidence
  for any future decision to promote travel from soft fact to opt-in
  rule. Fixture export: deterministic lines + tunings + bounds,
  archived with solver identity.

- **Problem B — complement pair cleanliness (existing rule set, exact
  contract)**: given part A fixed and a candidate rhythm for part B,
  decide whether any pitch assignment for B satisfies the *actual*
  `PairValidation` laws, or prove the window empty. The model is pinned
  to production semantics, not a convenient CSP cousin:
  - **B pitch domain (finite, named)**: the relation-mode domain the
    arranger actually draws from (normalized band / `ScaleLadder` per
    mode) — without a finite domain the solver trivially "solves" the
    problem by exiling B to a far register;
  - **playability**: A playable is an input *precondition*; B playable
    is a constraint under B's named tuning, `FingeringWeights::v1`,
    `STANDARD_MAX_FRET`, on the highest-pitch-per-onset line (a chord
    participates through its top note only) — `is_clean` requires both,
    and a Problem B without playability would be a fan adaptation of
    `PairValidation`, not `PairValidation`;
  - **coincident dissonance**: exact classes `{1, 6, 11}` mod 12 on
    coincident onsets;
  - **register mud**: the exact current law —
    `band_overlap > 0.5` over whole-part bands, relative to the
    narrower band, with the degenerate single-pitch rule;
  - **voice crossing**: an optional named extension (rule 15), never
    part of `PairValidation` v1.

Both problems archive exact inputs/outputs per the Lab's manifest
discipline; neither touches production (`deny.toml` unchanged).

## What the inventory deliberately does not contain

Workspace-level laws (forbid-unsafe, lint gates, TDD, English-only) —
they are repository constitution (SPEC/AGENTS), not
candidate-admissibility rules, and putting them in a `RuleScope` would
confuse two different kinds of "hard". Scoring axes (`rerank.rs`,
relation weights, `strong_beat_doubling_penalty`) — soft by definition,
catalogued where they live. Rules whose subject matter is deferred by
other documents (learned-model authority boundaries — the
preference-learning proposal's non-goals govern).
