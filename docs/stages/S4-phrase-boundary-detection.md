# S4: Phrase boundary detection

Status: planned
Depends on: S1
ADRs: ADR-0008, ADR-0010

## Goal

Detect phrase boundaries with explainable heuristics plus manual override.

## Inputs / Outputs

- In: canonical score (one or more tracks/voices).
- Out: `Vec<PhraseBoundary { start_tick, end_tick, score, reason }>`.

## Algorithm (baseline; calibrate on S5 corpus)

```
boundary_score(t) = w1·pause(t)        + w2·cadence(t)
                  + w3·rhythm_reset(t)  + w4·motif_boundary(t)
                  + w5·register_jump(t) + w6·density_change(t)
```

- Default weights equal (`w_i = 1/6`), overridable by config.
- Hard rules for obvious pauses / bar resolutions; soft score otherwise.
- A boundary is placed when `score > threshold` and distance to the
  neighbouring boundary exceeds `merge_if_shorter_than`.
- Quantize grid 1/16 default, 1/32 for tapping passages (density-detected).
- Manual override always wins and is recorded.

## Acceptance criteria

- On hand-labelled swancore phrases (from S5): F1 ≥ 0.7 at ±1/16 tolerance.
- Every boundary carries a `BoundaryReason`.
- Deterministic for the same input + config.
- P1 `phrase_boundary` fuzz target (structure-aware via `arbitrary`,
  ADR-0010): boundaries sorted, ticks within phrase duration, scores
  finite, `BoundaryReason` consistent with non-zero score components.

## Open questions

- Final default weights and threshold (placeholders until corpus exists).
- Cadence detection with poor harmonic context.

## See also

- [`../adr/0008-heuristic-phrase-detection-before-ml.md`](../adr/0008-heuristic-phrase-detection-before-ml.md)
- [`../glossary.md`](../glossary.md) §6
- [`../fuzzing.md`](../fuzzing.md) (`phrase_boundary`, P1)
