# S11: Region regeneration with frozen regions

Status: planned
Depends on: S10
ADRs: —

## Goal

Regenerate a selected range while preserving anchors and frozen regions.

## Inputs / Outputs

- In: a score + `RegenerationRegion` (a `TickRange`), `FrozenRegion`s,
  `AnchorPoint`s.
- Out: a regenerated region that respects locks and joins smoothly.

## Approach

- Generator accepts `Constraints { locked_ticks, locked_notes }`; never alters
  locked content.
- Condition on local context (previous + next phrase boundary).
- Continuity score across locked↔new joins (boundary intervals < 7 semitones,
  energy delta < ~0.3).

## Acceptance criteria

- Locked content is byte-stable across regeneration.
- Joins pass the continuity check; deterministic for a fixed seed.

## Open questions

- Continuity thresholds (calibrate on corpus).

## See also

- [`../glossary.md`](../glossary.md) §8 (Regeneration, Frozen region, Anchor)
