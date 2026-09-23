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
