# S6: Rule-based generator v0

Status: planned
Depends on: S5
ADRs: ADR-0005

## Goal

First musically useful, non-neural generator producing recognizably
swancore-like riffs.

## Inputs / Outputs

- In: corpus chunks, pitch material, constraints, `GenerationSeed`.
- Out: `Vec<GenerationCandidate>` over the canonical model (group/chord/voice
  aware).

## Strategies (priority order)

1. Rhythm-copy + pitch-substitute (rhythm from corpus, new pitches in key).
2. Motif-transpose + variation (transpose 3rd/5th/7th, invert contour).
3. Constrained random walk on scale degrees (penalize big leaps, repeats,
   off-style density).
4. Shuffle motifs grouped by tags.
5. Repeat + variation (call/response — replace last beat).
- Cadence-aware endings; anchor preservation; string/fret playability filter.

## Acceptance criteria

- Deterministic for a fixed seed (property test).
- Density within corpus mean ± 1σ; syncopation ≥ corpus lower quartile.
- 10 generated riffs: ≥ 60% judged "swancore-like" in a blind listen.

## Open questions

- Similarity/quality metric weights before S9 feedback exists.

## See also

- [`../glossary.md`](../glossary.md) §8
