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

