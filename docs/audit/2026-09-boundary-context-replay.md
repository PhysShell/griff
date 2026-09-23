# Boundary context replay — preregistered Lab experiment

Date: 2026-09-23
Status: preregistered before outcome

## Question and fixed population

Can the semantic context supported independently by #211 cross an unchanged
`TabLine` solver partition? The fixed population is the eight corrected
same-string legato relations whose origin and target survive in different kept
lines of the same imported `source + track + voice`. Excluded chord targets and
dropped-short-line targets are not repaired or pooled into this population.

Each case is keyed by source, track, voice, origin stable note id and target
stable note id. The runner fails closed unless there are exactly eight cases,
all targets are found exactly once in the retained line identified by the
forensic projection, and every observed origin/target pair is same-string.

## Frozen boundary contract and regimes

The experiment passes only:

```text
BoundaryContext {
    hand_state: target line's already-imported preceding anchor fret,
    incoming_technique: target stable atom must use origin imported string,
}
```

The target line, pitches, tuning, cut, `v1-fit` weights and human reference are
otherwise unchanged. Four deterministic exact-DP regimes are compared:

- **L0 independent:** target line under `v1-fit` alone.
- **LH hand:** L0 primary optimum, then lexicographically minimize summed
  fretted-note distance from the preceding anchor; open notes contribute zero.
- **LT technique:** L0 with the target stable note conditioned to the imported
  origin string.
- **LHT both:** LT primary optimum, then the same anchor secondary.

No lines are joined. No prior solver path is passed. No weight is fitted and no
production objective, importer, projection, `TabLine`, or public API changes.

## Measurements and falsifiers

For every regime record feasibility, primary optimum, optimum-set size and
human agreement floor/uniform/ceiling. For the deterministic path record whole
line and non-target note agreement; target agreement is mechanical under LT/LHT
and is reported separately. Also record whether the imported whole path belongs
to the primary optimum set.

This sample is technical replay evidence, not population statistics. The
boundary contract is technically supported when all eight cases replay without
identity loss, LT/LHT preserve the imported path's feasibility, and at least
one context regime changes a non-target or whole-line result without worsening
more cases than it improves. If context only fixes the constrained target and
never changes an independent metric, it is transport metadata but not evidence
for a richer boundary solve. Any missing/ambiguous target, same-string drift,
or change to the frozen 29,758 within-line / 46 cross-line census is a blocker.

The output is a licensed-corpus JSONL manifest plus aggregate JSON in the
disposable audit directory. Only this protocol and aggregate outcome may be
committed.

## Corpus outcome

The frozen runner was executed on the unchanged 410-file corpus fingerprint
`9e53e55a19cddf29`. It found exactly the preregistered eight kept-line
relations, across five song keys. Every target stable id resolved uniquely in
the projected retained line; all eight imported origin/target pairs remained
same-string. All target lines carried a preceding anchor. The independent
census remained 29,758 within-line and 46 cross-line relations.

The hard technique condition was feasible in `8/8`, and the complete imported
path remained feasible in `8/8`. Deterministic target agreement under
`L0 / LH / LT / LHT` was `4 / 6 / 8 / 8`. Thus the boundary relation transports
the exact target-string fact without joining solver partitions.

Against independent L0, deterministic agreement changes were:

| regime | whole line better / equal / worse | non-target better / equal / worse |
|---|---:|---:|
| LH hand | 2 / 5 / 1 | 2 / 5 / 1 |
| LT technique | 1 / 4 / 3 | 0 / 5 / 3 |
| LHT both | 3 / 4 / 1 | 2 / 5 / 1 |

The distinction matters. LT recovers all constrained targets, but its only
whole-line improvement is target-mechanical and it worsens three non-target
paths. On this population it is supported as exact transport metadata, not as
an independent preference over the rest of Line B. The hand channel changes
unconstrained choices with the preregistered positive sign. LHT combines the
two: target agreement is complete and non-target behavior retains the hand
result, producing the strongest whole-line split (`3/4/1`).

None of the eight complete imported paths belongs to the primary optimum set
under L0, LH, LT or LHT. The boundary contract therefore does not close the
remaining objective gap and does not justify promoting either channel into a
production objective. Anchor distance is only a lexicographic secondary here;
technique is only an observed hard condition.

## Verdict

The replay contract is **technically supported**: the same typed context that
survived the general #211 experiment crosses an ordinary `TabLine` partition
without identity loss, infeasibility, line joining, or production changes.
The evidence splits by channel:

1. `hand_state` has small positive independent replay evidence (`2/5/1`) on
   unconstrained notes;
2. `incoming_technique` is reliable target-string transport (`8/8`) but fails
   the richer-solve falsifier by itself (`0/5/3` non-target);
3. the combined contract is the minimal useful boundary representation for a
   future design experiment, not a validated production solver policy.

The sample is deliberately too small for corpus-level inference and is
concentrated: four of eight cases come from one transcription. The next step,
if taken, should be a design/ADR experiment specifying ownership, identity and
lifetime of `BoundaryContext`; it should not tune new weights on these eight
cases.
