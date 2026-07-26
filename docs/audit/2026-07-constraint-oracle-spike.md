# 2026-07 — Constraint Oracle Spike (MiniZinc offline oracle)

Executes the first increment of the hard-constraint-contract proposal:
one small fretboard problem and one complementary-guitar problem, modelled
through a solver-neutral IR, run through MiniZinc, with deterministic
fixture export and full input/output archival. Zero production
dependencies added; the `lab/` crate is excluded from the workspace
(`fuzz/` isolation precedent, ADR-0010) and CI never builds it.

## What was built

`lab/` (`griff-constraint-lab`, research tooling only):

- **solver-neutral IR** — finite sorted domains, three constraint kinds
  (`AbsDiffLe`, `ForbiddenIntervalClasses`, `BandOverlapAtMost`), stable
  FNV-1a problem fingerprints;
- **deterministic MiniZinc emission** — self-contained `.mzn`, byte-stable,
  witness printed as `name=value` lines;
- **exact reference solver** — deterministic backtracking with unary
  prefiltering and two sound band prunes, leaf-checked against the
  production `band_overlap` integer arithmetic (including the degenerate
  single-pitch rule);
- **runner** — emits fixtures, solves, cross-checks an external MiniZinc,
  archives one manifest per (fixture, solver) with solver identity.

TDD: the full contract suite (17 tests) was committed red first; the
green implementation followed; `griff-core` remained untouched and green.

## Problems modelled (per the Constraint Inventory)

- **Problem A — bounded-travel fretboard realization (experimental).**
  Fret domains from `Tuning::candidates`; `|fret_i − fret_{i+1}| ≤ bound`.
  Per the inventory's framing this is **not** a check on
  `infer_positions` (which exhaustively minimizes a weighted cost with
  travel soft by standing decision) and not the production reachability
  rule — it measures where a hypothetical hard bound would bite.
- **Problem B — complement pair cleanliness (existing rule set).**
  Part-A playability as a typed precondition; the B domain filtered to
  playable pitches (per-note existence — `is_playable`'s law for a
  monophonic line); production dissonance classes `{1, 6, 11}` on
  coincident onsets; the exact `band_overlap > 1/2` register-mud law.
  Contract tests reconstruct SAT witnesses into two-track scores and
  assert `validate_pair(..).is_clean()` — the oracle and the production
  validator agree by construction, not by hope.

## Results (archived under `lab/fixtures/` and `lab/runs/`)

Solvers: `griff-lab-exact` v1 (reference) and MiniZinc 2.8.7
(build 1478320236, chuffed backend). **All five fixtures: outcomes agree.**

| Fixture | Reference | MiniZinc/chuffed |
| --- | --- | --- |
| `bounded-travel-frontier-unsat` (bound 2) | UNSAT (26 nodes) | UNSAT |
| `bounded-travel-frontier-sat` (bound 3) | SAT (36 nodes) | SAT |
| `bounded-travel-loose` (bound 24) | SAT (8 nodes) | SAT |
| `pair-cleanliness-sat` | SAT (4 nodes) | SAT |
| `pair-cleanliness-mud-unsat` | UNSAT (12 nodes) | UNSAT |

**Calibration finding (Problem A).** The fixture line — an E-minor run
spanning `[E2 … B4]` (`40, 47, 52, 55, 59, 64, 67, 71`) — has its exact
travel frontier at **bound 3**: with free string choice, per-step fret
travel ≤ 3 suffices even across three octaves (witness
`0,2,2,5,4,5,8,7`), and bound 2 is provably UNSAT. A future opt-in travel
rule with any bound ≥ 3 would not reject lines of this shape; the bite
starts at ≤ 2. This is exactly the kind of evidence the inventory's rule 3
("calibration data, not code") calls for — and it cost five archived runs,
not a production rule.

**Honest-UNSAT finding (Problem B).** The mud fixture (no coincident
onsets, B domain trapped inside A's band) is UNSAT in both solvers — the
first machine-checked emptiness proof for a `PairValidation` window, the
use-case the Lab exists for.

## Limitations (recorded, not hidden)

- The reference solver is leaf-checked backtracking with two sound band
  prunes; adequate at spike size (≤ 36 nodes here), but a corpus-scale Lab
  needs real propagation.
- Problem A tracks fret distance only (mirroring `max_fret_jump`); string
  choice is unrecorded in the witness, and string-change cost is out of
  scope.
- One shared B domain per problem; relation-mode-specific per-onset
  domains are follow-up work.
- The external cross-check compares SAT/UNSAT status; witness equality is
  not required (both solvers' witnesses are verified against the
  constraints instead).

## Follow-ups proposed

1. Model the remaining Problem B extension (opt-in voice crossing,
   rule 15) once the rule is specified.
2. Chord-voicing feasibility (inventory rule 2) as the next oracle target
   when the ADR-0019 §7 model lands.
3. Replace leaf checks with propagation if fixture sizes grow past ~10⁷
   leaf visits — measure first (reachability-lab cost-benchmark
   discipline).
