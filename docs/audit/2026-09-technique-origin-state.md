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
