# Legato chord targets — preregistered preceding-context oracle

Date: 2026-09-22
Status: preregistered before the context corpus outcome

## Frozen population and baseline

This experiment reuses exactly the 38 chord-target relations and the exact
assignment oracle merged in PR #209. `B0` remains byte/value-equivalent to
that audit: v1 unary chord cost, pitch-correct candidates through fret 24,
distinct simultaneous strings, stable target identity, and the complete legal
target-string domain. Before interpreting context, the runner must reproduce
38 cases, 176 legal conditions, 174 feasible conditions, ranks `11/24/3`, and
free/costly/infeasible `11/27/0`.

The observed origin string is a label only. It is never a hard condition or a
term in a context score. No weight is learned or fitted on these cases.

## Context extraction

For each relation the artifact records the imported origin identity, onset,
pitch, original string/fret and tap mark separately from the latest
fretting-hand anchor strictly before the chord onset. Anchor selection combines
the existing semantics already used by the Lab:

- as in the tap-aware objective, tapped notes do not move the fretting hand;
- open strings do not establish a fret anchor;
- as in `TabLine::anchor_fret`, the latest qualifying onset wins and a chord at
  that onset contributes its lowest qualifying fret.

Absence is `None`; it is never replaced by zero. Origin and anchor may differ
when the relation skips intervening events or tapping leaves the fretting hand
in place.

## Registered exact views

For every admissible chord assignment and every legal target-string condition:

- `B0` is the unchanged sum of v1 unary costs;
- `O = abs(target_fret - origin_fret)`;
- `A = sum(abs(fretted_atom_fret - anchor_fret))`; open atoms contribute zero,
  exactly matching #199's `anchor_distance`. `A` is absent when the extracted
  anchor is absent.

Each scalar view is minimized exactly within a string condition and ranked
densely across feasible legal strings. Combined context has two symmetric,
predeclared sensitivity views rather than a selected winner:

1. exact lexicographic minimum `(O, A)` and its dense rank;
2. exact lexicographic minimum `(A, O)` and its dense rank.

When `A` is absent, anchor and combined ranks are explicitly absent. No scalar
`B0 + alpha*O + beta*A` is constructed.

The most important weight-free view is the global Pareto frontier over every
admissible assignment's `(B0, O, A)` tuple. A legal target string is a Pareto
member iff at least one assignment under that condition is not dominated by
an assignment from any legal target string. Pareto membership is not called
human optimality.

The complete artifact retains exact minima, counts and deterministic chosen
assignments where applicable, plus the full legal-string map. Imported human
voicings are scored descriptively only: B0/O/A values and excesses, and Pareto
membership. They are never used for fitting or as unique truth.

## Primary comparisons and evidence rule

For B0, O, A, O→A and A→O, report the observed dense rank. Relative to B0,
each available context view reports `rank_delta = base_rank - context_rank`
and `improved / unchanged / worsened`. Report all 38 cases and the 27 B0-costly
cases separately.

Cases are clustered, not independent. Per-song output includes cases, B0 and
context rank-1 counts, change counts and mean/median rank delta. Eleven
leave-one-song-out folds recompute aggregate rank-1 gain, change counts and
median delta without training.

Context is corpus-level candidate evidence only if observed rank improves more
often than it worsens, the gain is not created by one song, every leave-one-song
out aggregate keeps a positive sign, and the effect is not confined to one
repeated phrase family. No post-result threshold or objective repair is
allowed. A mixed or negative result is retained as such.

The complete legal-string domain plus song blocking is the registered negative
control. A matched alternative preceding note is not preregistered because the
repository has no canonical matching rule; manufacturing one would add an
outcome-sensitive context definition.

## Boundary

This is a Lab-only exact oracle. It changes no production path, importer,
`TabLine` slicing, Stage 2 objective or weights, `TechniqueEdge`, chord
representation, boundary state, learned tie-break, or corpus. The eight
kept-line relations remain outside this experiment.

