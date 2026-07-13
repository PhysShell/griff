# S6: Rule-based generator v0

Status: done
Depends on: S5
ADRs: ADR-0005, ADR-0010

> Progress (2026-07-11): the promised candidate *set* landed —
> `core/src/rerank.rs` fans a request over every strategy × seed variants and
> reranks on the closure + novelty axes under the `generation_rerank` v1
> policy (ADR-0017; melodic-closure note §7.2/§7.3). `griff generate` uses it
> by default, and `--corpus <dir>` feeds rhythm templates, novelty
> references, and the burst/rest gesture ask from curated chunks
> (decisions.log 2026-07-11).
>
> Post-acceptance hardening (2026-07-12): the register track is **accepted and
> closed**. `ScaleLadder` provides full-range reachability; Shuffle uses an
> unbiased ≤-octave `LadderWindow`; RhythmCopy traverses reflectively without
> wrapping; RepeatVariation chooses an endpoint local to the bar's actual
> penultimate degree. Focused corpus/synthetic validation found no remaining
> target-strategy >12-semitone jumps. A generic register rerank axis and global
> `RegisterPlan` were not justified and remain rejected/not started.
>
> Remaining follow-ups from the list below: anchor preservation, the string/fret
> playability filter, and the density/syncopation corpus gates. Cadence-aware
> endings are no longer owned as an unscoped S6 patch: they move to **S15 Phase
> 6**, after explicit tonal context, scope/confidence calibration, and an
> abstention path exist.

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
- Anchor preservation; string/fret playability filter.
- Cadence-aware endings are deferred to S15 Phase 6.

## Acceptance criteria

- Deterministic for a fixed seed (property test).
- Density within corpus mean ± 1σ; syncopation ≥ corpus lower quartile.
- 10 generated riffs: ≥ 60% judged "swancore-like" in a blind listen.
- P2 `generation_request` fuzz target (structure-aware, ADR-0010):
  arbitrary requests never panic; output duration == requested; all notes
  in range; fixed seed stays deterministic.

## Open questions

- Similarity/quality metric weights before S9 feedback exists.

## See also

- [`S15-tonal-context-and-harmonic-control.md`](S15-tonal-context-and-harmonic-control.md)
- [`../glossary.md`](../glossary.md) §8
- [`../fuzzing.md`](../fuzzing.md) (`generation_request`, P2)
