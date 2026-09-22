# Legato into chord targets — preregistered feasibility oracle

Date: 2026-09-22  
Status: preregistered before the corpus outcome

## Question and population

PR #208 found 38 resolved same-string legato targets that are note atoms of a
chord onset excluded from the monophonic `TabLine`. This experiment asks only
whether binding the identified target atom to the observed origin string is
compatible with a playable chord assignment and whether it changes the exact
optimum. The other eight cross-line targets are out of scope and remain the
population for a later boundary-state replay.

The target is identified by its stable imported-voice note id, never by pitch.
Duplicate pitches therefore remain distinct chord atoms.

## Registered model

The primary model is deliberately the smallest assignment model already
grounded by repository precedent:

1. each atom receives one position from `Tuning::candidates(pitch, 24)`;
2. two simultaneous atoms may not occupy one physical string;
3. every chosen position sounds the atom's exact pitch;
4. the constrained solve additionally requires the identified target atom's
   string to equal the imported legato origin string.

This is the assignment-only layer deferred by ADR-0019 §7 and named by
constraint-inventory rule 2. No fret-span, finger assignment, barre, reach,
hand-shape, or sequence law is promoted to a hard constraint: the repository
has no accepted simultaneous-hand model. Consequently `infeasible` means
infeasible under pitch/string assignment, not anatomically impossible.

The exact in-repo solver enumerates the finite candidate product with occupied
string pruning. Corpus chords have at most the tuning's string count, so this
is bounded and independently checkable. Atom and candidate order provide the
deterministic tie-break.

## Registered objective

Feasibility is primary and objective-independent. For feasible assignments the
ranking surrogate is the sum of ADR-0019 `v1`'s existing per-note term:

```text
sum(fret_weight * fret - open_string_weight when fret == 0)
```

with unchanged production `FingeringWeights::v1()` (`1, 1, 2, 1`). There is no
sequential transition inside a simultaneous chord, so the transition terms are
not repurposed. No weight is fitted on the 38 cases. A result may motivate a
later registered sensitivity analysis, but cannot alter this primary result.

## Solves and controls

For every case:

- **U** enumerates every admissible chord assignment;
- **C** adds `target_atom.string == observed_origin_string`;
- one exact conditional solve is also run for every legal string of the target
  pitch, producing the complete target-string feasibility/cost/count map.

The observed string's rank orders feasible conditions by optimum cost and then
string number. Equal-cost strings share the same dense cost rank; the artifact
also records how many legal strings tie for best. This complete-domain control
is the negative control—no random alternative is sampled.

When every imported chord atom has a usable explicit position, that realization
is checked for pitch correctness, fret range and string exclusivity, then
scored. Membership and excess are reported against both U and C. Imported
voicing is observed evidence, not a hard truth or unique target.

## Registered outcomes

- **feasible and free**: C is feasible and `cost(C) == cost(U)`;
- **feasible but costly**: C is feasible and `cost(C) > cost(U)`;
- **infeasible**: C has no assignment.

Primary counts are C feasibility, free/costly/infeasible, `delta_cost`, optimum
set reduction, observed-string dense rank and best-tie size, plus human
feasibility/constraint/membership/excess. Descriptive strata are boundary cause,
direction, origin tap, chord size and GP family. Tiny strata do not support
strong claims.

## Decision rule

The relation is a candidate chord-voicing signal only if most observed
constraints are feasible, observed strings are systematically among the
cheapest legal target strings, a non-trivial share changes cost or optimum set,
and imported realizations agree with C beyond what complete legal-string ties
explain.

If constraints are almost always free, observed strings look like arbitrary
legal alternatives, or feasibility often fails without a model-grounded
explanation, these 38 cases are not evidence for chord-aware boundary
architecture. Even a positive result proves at most that observed legato into
chords carries fingering information lost by the monophonic Lab
representation; it does not select a production solver or global architecture.

## Corpus result

The registered runner was executed over the same 410-file corpus as Stage 2
(fingerprint `9e53e55a19cddf29`; 3 import failures, 1,149 guitar tracks). It
recovered exactly the registered 38 relations. Generated JSONL and summary
artifacts stayed under the disposable `--out` directory and are not committed.

| result | cases |
|---|---:|
| constrained assignment feasible | 38 / 38 |
| feasible and free | 11 / 38 |
| feasible but costly | 27 / 38 |
| infeasible | 0 / 38 |

The constrained-minus-unconstrained cost has min / median / p90 / max
`0 / 6 / 11 / 19`. In 27 cases the observed-string condition removes every
member of U's optimum set. Across all 38 cases the median reduction of U's
original optimum set is one assignment (p90 and max four). This is measured as
survival of U-optimal assignments, rather than subtracting C's optimum count
from U's: those cardinalities describe different cost levels whenever C is
costly and are not comparable.

All full legal-string controls were enumerated. There are 176 legal target
string conditions and 174 are assignment-feasible. The two infeasible
alternatives are strings 5 and 6 of the same `Wolf & Bear - Street Rat` chord;
the observed string 4 is feasible. The observed condition's dense cost rank is:

| rank among feasible legal target strings | cases |
|---|---:|
| 1 | 11 |
| 2 | 24 |
| 3 | 3 |

Only 2/38 observed strings are uniquely cheapest. At the observed cost, 25
cases have no string-cost tie, 8 tie with one alternative, and 5 tie with two.
Thus the result is not explainable by universal legal-string symmetry, but the
registered unary surrogate usually prefers a different target string.

Every chord has a complete imported realization. All 38 are feasible under the
assignment model and all 38 satisfy the observed constraint. The latter is not
independent validation: both the constraint and the imported realization come
from the same imported string identity. Imported membership is 6/38 in U's
optimum set and 12/38 in C's. Human excess over U has median 16 and max 28;
excess over C has median 10 and max 20. Conditioning therefore moves the
surrogate toward the imported realization, but still leaves 26/38 imported
voicings outside C's optimum set.

Descriptive strata, with `free / costly`, are:

| stratum | cases |
|---|---:|
| chord-only | 10 / 25 |
| rest + chord | 1 / 2 |
| ascending | 9 / 5 |
| descending | 2 / 8 |
| unison | 0 / 14 |
| chord size 2 | 5 / 1 |
| chord size 3 | 5 / 18 |
| chord size 4 | 1 / 8 |
| untapped origin | 11 / 25 |
| tapped origin | 0 / 2 |
| GP3–5 | 10 / 24 |
| GP6/7 | 1 / 3 |

These 38 relations are not 38 independent song samples. They come from 11 song
keys (35 train, 3 holdout); the three largest songs contribute 23 cases.
`Cream of the Crop` contributes 12/12 costly cases, `Betrayed by the Game`
6/6 costly, while `Connector` contributes 5/5 free and `Polka Dot Dobbins`
4/4 free. The apparent direction and chord-size differences are therefore
descriptive only and may be phrase/song effects.

## Outliers and forensic follow-up

The maximum delta, 19, is the size-3 chord at tick 142080 in
`Dance Gavin Dance - Its Safe To Say You Dig The Backseat.gp5`
(`f107.t1.v0.at107520`). Its observed string has dense rank 3, while the
imported realization is exactly C-optimal. The other rank-3 conditions are two
relations in `Dance Gavin Dance - Frozen One.gp5`, with deltas 11 and 10. The
two repeated `Blood Wolf` relations each have delta 14 and rank 2. These cases,
plus the two infeasible *alternative* strings in `Street Rat`, are the small
forensic set for a context-conditioned follow-up. There are no observed-C
infeasible cases to explain.

## Interpretation against the registered rule

H2's assignment-level failure mode is falsified for this corpus: preserving the
observed origin string is compatible with an exact pitch-correct, distinct-
string chord assignment in 38/38 cases. H0 in its simple "free constraint"
form is also unsupported: 27/38 conditions raise the registered optimum and
remove all U-optimal assignments.

The full H1 decision rule is not met. Although the relation is feasible and
often consequential, the observed string is cheapest in only 11/38 cases and
uniquely cheapest in 2/38. The complete-domain control therefore does not show
that the observed string is systematically preferred by the registered
standalone chord surrogate. The honest result is mixed: the import contains a
real, losslessly identifiable chord-target string condition, but this
experiment does not establish that the condition alone improves chord voicing
under a context-free unary objective.

The failure location is representational rather than a kept-line boundary
replay problem: all 38 targets are atoms of chord onsets deliberately absent
from the monophonic `TabLine`. A boundary state could not constrain an atom the
model does not contain. This says only that a future chord-aware experiment
must represent the atom; it is not evidence to change slicing or production
architecture. The separate eight non-chord cross-line relations remain out of
scope for boundary-state replay.

## Limitations and next falsifiable experiment

The hard layer checks pitch, fret range and one-note-per-string assignment, not
anatomical hand shape. The objective is the preregistered sum of existing v1
unary fret/open costs; it contains no chord span, barre, preceding anchor or
voice-leading term. The corpus is small and song-clustered, and human
constraint agreement is mechanically coupled to how the constraint was
observed. No weight was tuned and no post-result sensitivity sweep was used to
rescue the hypothesis.

The next experiment should remain Lab-only and preregister a
context-conditioned exact control for these same 38 cases: add the existing
preceding-line anchor/origin state as a separately reported ranking layer,
enumerate the same complete target-string domain, and evaluate song-blocked
rank changes (including leave-one-song-out), especially on the rank-3 and
high-delta forensic set. The falsifiable question is whether context moves the
observed string toward rank 1 across song groups without fitted weights. If it
does not, these cases should remain import-preservation metadata rather than an
objective signal. No production chord implementation follows from this audit.

## Invariance and validation

The final rerun retained 29,758 within-line legato relations and 46 resolved
cross-line relations. The registered `v1-fit` Stage 2 values are unchanged:

| stage | exactness | gap to untapped baseline |
|---|---:|---:|
| B tap-aware | 0.9% | 19.1 pt |
| C1 hard continuity | 13.3% | 7.6 pt |
| D1 + pull-off open waiver | 16.8% | 4.2 pt |

The Lab release test suite and strict all-target clippy gate pass offline. The
runner changes no production objective, weight, importer, slicing rule,
`TechniqueEdge`, or chord generator.
