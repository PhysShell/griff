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

## Corpus result

The registered runner was executed over the unchanged 410-file corpus
(fingerprint `9e53e55a19cddf29`; 3 import failures, 1,149 guitar tracks).
Generated manifests remain under the disposable `--out` directory.

### B0 reproduction

The runner refuses to emit a context summary unless every frozen #209 census
value matches. The rerun passed:

```text
cases                         38
legal target-string conditions 176
feasible conditions            174
B0 rank 1 / 2 / 3           11 / 24 / 3
free / costly / infeasible  11 / 27 / 0
```

All 38 cases have a preceding fretting-hand anchor under the preregistered
tap-aware extraction. No missing context was replaced by zero.

### Observed-string ranks

| view | rank distribution | rank 1 | improved / same / worse | median rank delta |
|---|---|---:|---:|---:|
| B0 | 11 / 24 / 3 at ranks 1 / 2 / 3 | 11 | — | — |
| O | 36 at rank 1; 2 at rank 3 | 36 | 26 / 11 / 1 | +1 |
| A | 30 / 3 / 3 / 2 at ranks 1 / 2 / 3 / 4 | 30 | 22 / 13 / 3 | +1 |
| O→A | 36 at rank 1; 1 at rank 3; 1 at rank 4 | 36 | 26 / 11 / 1 | +1 |
| A→O | 30 / 3 / 2 / 3 at ranks 1 / 2 / 3 / 4 | 30 | 22 / 13 / 3 | +1 |

The symmetric lexicographic views do not require a fitted coefficient. O→A
has the same aggregate rank-1 and change counts as O; A→O has the same counts
as A, although individual lower-rank cases can move within those totals.

Every observed target string has at least one globally nondominated assignment
on `(B0, O, A)`: 38/38 Pareto members. This is supportive but weakly
discriminating—trade-offs allow many conditions onto a frontier—so the dense
rank and song-blocked results carry the interpretation.

### The 27 previously costly cases

| view | rank 1 | improved / same / worse |
|---|---:|---:|
| O | 26 / 27 | 26 / 1 / 0 |
| A | 21 / 27 | 22 / 4 / 1 |
| O→A | 26 / 27 | 26 / 1 / 0 |
| A→O | 21 / 27 | 22 / 4 / 1 |

This is the population on which preceding context had to do work rather than
merely retain a B0 winner. Both registered primitive views show a signal.

### Song distribution

The table reports `B0 rank-1 → O rank-1 / A rank-1` and, in parentheses,
`improved / worsened` for O and A.

| song key | cases | result |
|---|---:|---|
| A Lot Like Birds — Connector | 5 | `5 → 5 / 5` (`0/0`, `0/0`) |
| Dance Gavin Dance — Betrayed by the Game | 6 | `0 → 6 / 6` (`6/0`, `6/0`) |
| Dance Gavin Dance — Blood Wolf | 2 | `0 → 2 / 2` (`2/0`, `2/0`) |
| Dance Gavin Dance — Blue Dream | 1 | `1 → 0 / 0` (`0/1`, `0/1`) |
| Dance Gavin Dance — Cream of the Crop | 12 | `0 → 12 / 12` (`12/0`, `12/0`) |
| Dance Gavin Dance — Frozen One | 2 | `0 → 2 / 0` (`2/0`, `1/0`) |
| Dance Gavin Dance — Its Safe To Say You Dig The Backseat | 1 | `0 → 0 / 0` (`0/0`, `0/0`) |
| Dance Gavin Dance — Polka Dot Dobbins | 4 | `4 → 4 / 4` (`0/0`, `0/0`) |
| Underoath — Reinventing Your Exit | 1 | `0 → 1 / 0` (`1/0`, `0/0`) |
| Wolf & Bear — Street Rat | 2 | `0 → 2 / 1` (`2/0`, `1/0`) |
| Wolf & Bear — There's No Dust in the City | 2 | `1 → 2 / 0` (`1/0`, `0/2`) |

O improves cases in seven song keys and A in five, so the result is not
confined to one repeated phrase family. Concentration remains material:
`Cream of the Crop` contributes 12/26 O improvements and 12/22 A improvements.
It does not create the sign by itself.

### Eleven-way leave-one-song-out

All 44 registered fold/view records are emitted. Their ranges are:

| view | rank-1 gain, min … max | min `(improved − worsened)` | fold medians |
|---|---:|---:|---|
| O | +13 … +26 | +13 | +1 in every fold |
| O→A | +13 … +26 | +13 | +1 in every fold |
| A | +7 … +20 | +7 | 0 or +1 |
| A→O | +7 … +20 | +7 | 0 or +1 |

In particular, omitting the largest song leaves rank-1 gains of +13 for O and
+7 for A. No omission reverses either rank-1 gain or the change-count sign.

### Imported realization

The imported target fret has zero O excess in 38/38 cases. This is mechanical,
not independent validation: within a fixed target string and tuning, the
target pitch fixes its fret, and the import supplies both the label and the
realization. Anchor excess is zero in 27/38 cases (median and p90 zero, maximum
22). The full imported assignment is globally Pareto-nondominated in 18/38
cases. Human measurements remain descriptive and were not used by any solve.

## Registered forensic cases

| case | B0 delta/rank | O | A | O→A | A→O | observed Pareto |
|---|---:|---:|---:|---:|---:|---|
| `Its Safe To Say…`, tick 142080 | 19 / 3 | 3 | 3 | 3 | 3 | yes |
| `Frozen One`, tick 288000 | 11 / 3 | 1 | 3 | 1 | 3 | yes |
| `Frozen One`, tick 391680 | 10 / 3 | 1 | 2 | 1 | 2 | yes |
| `Blood Wolf`, tick 15480 | 14 / 2 | 1 | 1 | 1 | 1 | yes |
| `Blood Wolf`, tick 134520 | 14 / 2 | 1 | 1 | 1 | 1 | yes |
| `Street Rat`, tick 286080 | 5 / 2 | 1 | 1 | 1 | 1 | yes |
| `Street Rat`, tick 298080 | 5 / 2 | 1 | 2 | 1 | 2 | yes |

The maximum-delta `Its Safe To Say…` case is a useful falsifier: neither
registered context primitive improves it. The two `Frozen One` cases separate
the primitives cleanly—origin proximity explains both, anchor proximity does
not fully do so. Both repeated `Blood Wolf` cases improve under both views. The
two assignment-infeasible *alternative strings* in the first `Street Rat`
chord remain present in the complete control map; the observed condition is
feasible and context-favoured.

## Verdict against the preregistered rule

The result qualifies as **corpus-level candidate evidence** that #209 was
context-starved:

1. O improves 26 and worsens 1; A improves 22 and worsens 3.
2. Removing the largest song leaves positive rank-1 gains and positive net
   change counts.
3. Every one of the eleven leave-one-song-out folds retains a positive sign.
4. Improvements occur across seven song keys for O and five for A, rather than
   one repeated phrase family.

The stronger statement is supplied by A: it sees only the previously available
fretting-hand anchor and the chord assignment, not the observed string label,
yet moves rank 1 from 11/38 to 30/38. O is even stronger at 36/38, as expected
for a metric that tests whether the imported same-string relation corresponds
to a small physical fret continuation. Neither uses a reward for string
equality, and no weights were fitted.

This does **not** select a production architecture or scalar objective. O is
not independent of the relation's imported provenance, the sample remains only
11 song keys, Pareto membership is insufficiently selective, and A still
worsens three cases. The next falsifiable step, if pursued, should be a new
preregistered chord-aware representation experiment that preserves these
primitive dimensions without combining them into fitted weights. The eight
kept-line targets and boundary-state replay remain separate.

## Invariance

The final corpus rerun retains 29,758 within-line and 46 resolved cross-line
relations. `v1-fit` Stage 2 remains B/C1/D1 = `0.9 / 13.3 / 16.8%`, with gaps
`19.1 / 7.6 / 4.2` percentage points. No production or Stage 2 behavior changed.
