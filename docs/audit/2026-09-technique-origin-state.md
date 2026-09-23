# Technique-origin performance-state recovery — preregistration

Date: 2026-09-23  
Status: preregistered before implementation or corpus outcome  
Exact base: `f50e2270ce87df733343bf2649179fb21169d6ff`

## Question and scope

Can a causal estimator recover the imported string of a technique-bearing
origin note from the musical score and available past, without reading the
imported realization of either endpoint? A second, explicitly conditional
question asks how recoverability changes when an upstream technique planner
supplies target identity, pitch and onset.

The two information regimes are reported separately:

- **BLIND:** no target identity, pitch, onset or realization;
- **INTENT_AWARE:** target stable identity, pitch, onset and meaningful
  relation kind, but never target string or fret.

This Lab-only experiment studies the state-estimation step before
`BoundaryContext`. It does not change that contract, production crates,
weights, line slicing, relation projection or the corpus. It does not promote
an objective, a hard obligation, an ADR, an HMM or a learned model.

## Prior art

- Itoh and Hayashida (2004), *Optimization for Guitar Fingering on Single
  Notes*, formulates string/fret/finger selection as multistage dynamic
  programming with explicit motion costs and deterministic preference levels.
- Hori, Kameoka and Sagayama (2013), *Input-Output HMM Applied to Automatic
  Arrangement for Guitars*, treats left-hand forms as hidden sequential state
  decoded by an HMM/Viterbi formulation.
- Edwards et al. (ISMIR 2024), *MIDI-to-Tab: Guitar Tablature Inference via
  Masked Language Modeling*, assigns strings using full-sequence learned
  context.
- `jgollub1/guitar_dp` is an MIT-licensed monophonic dynamic-programming
  reference over note/finger states.

These are conceptual references only. No code, dependency or fitted model is
introduced. The present experiment first identifies which transparent
information channel is missing; structured latent-state or learned sequence
models are possible later comparisons only if these regimes fail.

## Frozen population and identities

The corrected relation census on the base commit is invariant:

```text
within-line corrected same-string relations: 29,758
resolved cross-line relations:                   46
total:                                       29,804
cross-line excluded-chord targets:               38
cross-line kept-line targets:                      8
```

The primary population is every resolved relation whose origin has stable
identity, valid pitch, explicit imported position for evaluation and is visible
to the retained solver representation. A row key contains source, song key,
track, voice, origin and target stable ids, and both onsets. Origins are first
classified as `UniqueOrigin`, `MultipleTargetsSameOrigin`, `MissingOrigin` or
`Unsupported`; multiple relations from one origin are not silently counted as
independent decisions. The full census and refusal reasons are retained.

The eight kept-line and 38 chord-target cross-line cases are frozen downstream
cohorts, not development sets. The run must fail closed on census or identity
drift. The expected corpus has 410 files and fingerprint `9e53e55a19cddf29`.

## Information boundary

Estimator input and evaluation labels are different types. Conceptually:

```text
TechniqueOriginProblem        ObservedOriginRealization (label only)
  identity                      string
  tuning                        fret
  max_fret
  origin_pitch               ObservedTargetRealization (evaluation only)
  permitted score/past         string
  optional technique intent    fret
```

No estimator feature may inspect imported origin string/fret, imported target
string/fret, or equality with either. BLIND cannot contain target identity or
pitch. INTENT_AWARE may contain target identity/pitch/onset because an upstream
planner could supply them, but the corrected target identity itself came from
imported technique provenance; therefore every positive intent result is
strictly conditional on that upstream information. Raw technique kind is
reported as degenerate if the corpus cannot distinguish it.

## Frozen objective landscape

`B0` is unchanged `v1-fit`, with its deterministic chosen path. For every
legal origin string `s`, the experiment conditions only that stable origin note
to `s` and exactly resolves the frozen line problem:

```text
cost(s), global optimum, delta(s), primary-optimal string count,
imported-string delta, dense rank, optimum membership, unique optimality
```

This separates deterministic tie failure (`delta = 0`, wrong chosen string)
from primary-objective failure (`delta > 0`). The conditioned solver is checked
against independent brute force on tiny fixtures.

## Registered transparent regimes

- **V0 / B0:** frozen deterministic `v1-fit` baseline.
- **V1:** complete conditioned-string B0 profile (BLIND diagnostic).
- **V2 / T:** INTENT_AWARE origin domain retains string `s` only when both
  origin and target pitches are playable on `s` within `max_fret`; target state
  is not coupled and imported labels are not inspected.
- **V3 / T+H-P:** T plus lexicographic imported-past hand distance, using the
  retained partitions and producer semantics. Current origin realization is
  hidden. This is a teacher-forced upper bound.
- **V4 / T+H-C:** T plus the same lexicographic hand secondary, with past
  positions produced endogenously by frozen solver output.
- **V5 / J:** within-line only, the existing corrected C1 stable-identity
  constraint `origin.string == target.string`.
- **V6 / J+H-P** and **V7 / J+H-C:** included only if the existing helper admits
  the same hand secondary without new semantics.

`T` and `J` are different: T only prunes origin strings by pitch reachability;
J jointly couples the two solver states. No scalar combination or fitted
coefficient is permitted.

Hand state uses the established causal semantics: tapped notes do not move the
fretting hand, open notes do not establish a fret anchor and the latest
qualifying event wins. `H-P` and `H-C` share retained partitions and producer
semantics. Full-import `H-F` may be reported only as a descriptive
representation upper bound. Unknown remains `Unknown`, never zero or absent.
For known hand fret `h`, the secondary is
`abs(origin_fret(s) - h)`, ordered lexicographically after frozen primary cost;
the primary/secondary Pareto set is also reported when inexpensive.

Every regime produces a typed estimate:

```text
Known { string } | Ambiguous { strings } | Unsupported { reason }
```

`Known` requires a unique winner under the registered rule. Equivalent strings
remain an ordered `Ambiguous` set; deterministic low-string order must not
masquerade as certainty. Only `Known` could ever be considered for a future
hard `required_string`.

## Metrics and robustness

BLIND and INTENT_AWARE are never combined into one accuracy. Relation-level
metrics are deterministic exactness, imported-string optimum membership,
dense rank, primary delta, Known coverage/precision, Ambiguous membership and
set size, and Unsupported counts/reasons. T additionally reports raw versus
technique-feasible domain size and verifies that every valid imported string
is retained. J reports the same origin metrics separately.

Technique origins are compared descriptively with other notes on the same
retained lines under frozen B0. This is a paired context, not a formal matched
control population.

All summaries are case-weighted and macro-by-song. Per song report relation
count, V0/V2/V4 exactness and ranks, Known coverage/precision, and
improved/same/worse. Leave-one-song-out reports effect sign, min/max omission
effect and the largest song contribution; no random split is used and no model
is trained.

A deterministic, non-fitting repetition signature uses origin pitch class,
target pitch interval, relative onset gap, local pitch interval before origin,
local pitch interval after target when available, and pitch-derived technique
direction. It excludes positions. Report unique signatures, largest count and
one row per `(song, signature)` as a concentration diagnostic, not a primary
gate.

For the pinned eight kept-line cases, each regime reports estimate type,
origin agreement, emitted required string only when Known, target-line
feasibility, target agreement, whole-line agreement and non-target agreement.
For the 38 chord-target cases, reuse the existing chord-event representation
without changing its model and report Known coverage, origin correctness,
target constraint feasibility and whether the imported chord remains feasible.
Neither replay exposes target realization to the estimator.

## Evidence rules and falsifiers

- **A — tie-break problem:** supported when imported strings often have zero
  primary delta while V0 chooses another, robustly across songs.
- **B — primary-objective problem:** supported by a substantial cross-song
  positive-delta distribution. No post-outcome percentage threshold is added.
- **C — target intent informative:** supported only when T/J improves
  rank/exactness across multiple song keys and leave-one-song-out never reverses
  the aggregate sign. The conclusion remains conditional on upstream intent.
- **D — hand adds independent signal:** supported only when T+H improves over T
  and the H-P effect does not disappear under H-C. Teacher-forced to endogenous
  degradation is always reported.
- **E — safe hard-obligation candidate:** any wrong Known string rejects a
  universal hard constraint. Ambiguous and Unsupported are never collapsed.

A T row that removes its valid imported string is a forensic blocker. Corpus,
Stage-2 or production-behavior drift is a blocker. Transparent failure is a
result, not permission to tune weights, fit a hand coefficient, add exceptions,
special-case pinned cohorts, train a model or read imported target positions.

## Tests and artifacts

Contract tests cover structural label separation; BLIND/INTENT type access;
conditioned cost versus brute force; zero-delta wrong deterministic tie;
positive delta and dense rank; T reachability without target labels; J stable
identity and duplicate pitches; H-P exclusion of the current label; H-C use of
past endogenous outputs only; Unknown preservation; typed ambiguity;
deterministic permutation/tuning orientation/song/signature summaries; pinned
8/38 cohorts; and the `29,758 / 46` census.

Disposable full rows are written under `--out` as census/profile JSONL plus
summary, song, LOO, signature, boundary-eight and chord-38 JSON. Only compact
summary/audit artifacts are committed, consistent with corpus licensing.

Validation is frozen to formatting, release offline tests and clippy, the full
410-file corpus, invariant fingerprint/census, and unchanged Stage-2 B/C1/D1
rates (`0.9 / 13.3 / 16.8%`) and gaps (`19.1 / 7.6 / 4.2`). Runtime and exact
search cost are reported.

The strongest possible conclusion is narrow: under a specified information
regime, transparent causal state recovers a useful fraction of
technique-origin strings without imported endpoint realizations. It does not
establish a production objective, global solver, automatic technique planner,
BoundaryContext promotion or learned architecture.

## Corpus outcome

The registered runner completed on all 410 files (three import refusals),
fingerprint `9e53e55a19cddf29`. The frozen census remained 29,758 within-line
plus 46 cross-line relations. All 29,804 relations had a distinct stable
origin; there were no multiple-target, missing-origin or unsupported
refusals. Every imported string survived T, so the intent domain passed its
forensic gate. The amended exact profiling and all downstream artifacts took 244 seconds
after compilation.

### BLIND objective diagnosis

Frozen V0 chose the imported origin string in `11,945 / 29,804` cases (40.1%).
The complete primary landscape is more informative:

```text
imported string primary-optimal: 15,327 / 29,804 (51.4%)
positive primary delta:          14,477 / 29,804 (48.6%)
zero-delta but wrong V0:          3,382 / 29,804 (11.3%)

dense rank 1 / 2 / 3 / 4 / 5 / 6:
15,327 / 9,321 / 3,854 / 1,153 / 127 / 22

primary delta p50 / p90 / max: 0 / 6 / 33
```

Thus the residual is not mainly a deterministic tie-break bug. Tie state is
missing in 3,382 cases, but in nearly half the population the primary objective
strictly prefers a different origin string.

The typed BLIND estimate was Known for 18,878 cases and correct for 7,523
(39.9% precision); 10,926 were Ambiguous, with the imported label present in
7,804 sets. This deliberately differs from merely calling V0's deterministic
low-index choice “known”.

The descriptive same-line comparison does not reveal a technique-only
catastrophe: V0 string agreement was `11,945 / 29,804` (40.1%) on origins and
`43,308 / 102,077` (42.4%) on other notes in the same retained lines. Origins
are somewhat harder, but the objective mismatch is broad.

### Conditional target intent

T reduced the aggregate legal domain from 122,826 to 111,032 strings (4.12 to
3.73 per origin) without inspecting or removing any imported label. Its
deterministic exactness rose to `13,441 / 29,804` (45.1%): 1,496 paired
improvements, 28,308 equal and zero worse than V0. Per-song signs were 69
better, 117 equal and zero worse across 186 song keys. Macro-by-song exactness
rose from 43.5% to 48.0%; every leave-one-song-out aggregate effect stayed
positive.

This supports target-pitch feasibility as information, but only conditional on
an upstream planner already supplying the corrected target identity and pitch.
It is not BLIND recovery.

Joint C1/J, evaluated only on 29,758 within-line relations, reached 12,245
deterministic exact origins (41.1%). It guarantees endpoint equality, not the
imported choice among several jointly feasible strings, and is much weaker for
origin recovery than T. Same-string coupling and origin-string estimation are
therefore different questions.

T's typed estimate was Known in 19,511 cases and correct in 8,796 (45.1%
precision); 10,293 remained Ambiguous, with 7,658 imported-label memberships.
Any wrong Known result fails the preregistered universal-hard-obligation gate.

### Hand state: teacher-forced versus endogenous

With the retained-partition imported past, T+H-P reached 14,613 deterministic
exact cases. Relative to T this is `1,761 better / 27,454 equal / 589 worse`.
It made 28,978 estimates Known, 14,284 correct (49.3% precision), and left 826
Ambiguous.

With the same producer semantics but frozen solver past, T+H-C fell to 13,360
exact: `242 better / 29,239 equal / 323 worse` relative to T, and 1,253 fewer
exact cases than H-P. It made 28,403 estimates Known, 12,818 correct (45.1%
precision), and left 1,401 Ambiguous.

Teacher-forced hand state therefore contains independent preference signal,
but it does not survive endogenous replay. Evidence rule D fails: current past
state estimation, rather than transport, erases the gain. H-C is slightly
worse than T both case-weighted and macro-by-song (48.0% to 47.7%).

### Concentration and robustness

The largest song contributed 1,075 of 29,804 relations. Omitting any one song
left the H-C versus V0 exact-count effect positive (`+1,229` to `+1,429`). The
descriptive signature audit found 4,719 unique signatures and 5,450
`(song, signature)` rows; the largest such cell contained 110 relations. On one
representative per cell, exact counts were V0 2,159, T 2,405 and H-C 2,378.
The T result is therefore not one song or one repeated phrase. The very small
H-C-over-T aggregate does not survive this concentration view.

### Pinned downstream cohorts

For the eight kept-line boundary cases, BLIND and T were Known in 4/8; H-P and
H-C were Known in 8/8. Every emitted constraint was feasible, but every regime
recovered the observed target string in only 1/8. The old #213 endogenous
fidelity result is reproduced: extra certainty did not become extra accuracy.

For the 38 excluded chord-target cases, H-C was Known in 24/38. All 24 emitted
target constraints were exact-chord feasible and the imported chord itself
remained physically feasible, but only 9/24 estimated origin strings were
correct and only those same nine imported chords satisfied the estimated
constraint. This also rejects production hard obligations.

### Deterministic-reporting amendment

Review of the first outcome found that its deterministic T/H counts selected
the first canonically ordered string from an `Ambiguous` estimate. That was a
valid set representative but not a measurement of the full-chain solver's
actual tie behavior. The profile, ranks, deltas and typed Known/Ambiguous
metrics were unaffected.

Before rerun, `Chain::restrict_note_strings` was added to preserve every
surviving candidate and its unary/pairwise edges. Deterministic T is now the
origin position from `lexicographic_path` on that restricted chain. H-P/H-C use
the same chain with the registered anchor-distance secondary. Tests compare the
reported result differentially with direct restricted DP across more than one
thousand generated tied profiles. Solver behavior and epistemic certainty are
now independent outputs.

The fresh rerun left T exactly unchanged at 13,441 and retained its
`1,496 better / 28,308 equal / 0 worse` comparison with V0. The hand counts did
change: H-P moved from the withdrawn 14,729 to 14,613, and H-C from 13,394 to
13,360. All figures elsewhere in this outcome use the amended actual-DP
reporting. The primary diagnosis and hard-obligation rejection remain
unchanged.

### Invariance and validation

Release offline tests, formatting and all-target clippy passed. The independent
Stage-2 rerun preserved the registered v1-fit slice exactly:

```text
B / C1 / D1 exactness: 0.9% / 13.3% / 16.8%
gaps to matched baseline: 19.1 / 7.6 / 4.2 points
```

No production behavior, projection, census, corpus fingerprint or
`BoundaryContext` contract changed.

## Verdict against preregistered evidence rules

1. **A — tie-break problem: partially supported, not dominant.** There are
   3,382 zero-delta wrong deterministic choices, but 14,477 positive-delta
   failures.
2. **B — primary-objective problem: supported.** Positive primary delta is
   large and distributed across songs; changing only deterministic tie order
   cannot recover these labels.
3. **C — target intent informative: supported conditionally for T.** T improves
   1,496 cases, worsens none, is positive across 69 song keys and never reverses
   under leave-one-song-out. J shows that hard joint equality alone does not
   identify the imported origin string.
4. **D — hand adds independent causal signal: rejected.** H-P is positive, but
   H-C loses the gain and is 47 cases worse than T overall.
5. **E — safe hard obligation: rejected.** Every transparent regime emits many
   wrong Known strings; the pinned 8/38 cohorts demonstrate the downstream
   consequence directly.

The narrow answer is that current origin errors are a mixture of missing
secondary state and, more often, a genuinely wrong primary objective. Upstream
target pitch supplies useful causal domain information, but the minimum tested
regime is not sufficient for safe origin realization: endogenous hand state
does not preserve the teacher-forced benefit. A later comparison may now
legitimately study richer structured latent state (the HMM/hand-form direction)
or a song-held-out learned sequence model, but this PR adds neither.
