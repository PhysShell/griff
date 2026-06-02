# S14: Structure controls and metrics

Status: planned
Depends on: S6 (rule generator), S4 (phrase boundaries, for `phrase_length`)
ADRs: ADR-0015

> Roadmap note: appended as the next free stage number (append-only, per the
> stage-label history in [`../audit/`](../audit/)). Logically it sits beside the
> single-part generator (S6) and region regeneration (S11) and *feeds* the graph
> layer (S7); it deliberately does **not** depend on S7 or DP/Viterbi (ADR-0013).

## Goal

Make the time-organisation of generated material a first-class, controllable
thing — separately from "complexity". Distinguish four axes that today collapse
into one knob:

- **target span** — how much of the timeline to fill (already a `TickRange`);
- **pattern period** — how long the repeating idea is (may be sub-bar);
- **repeatability / variation** — how strictly the idea repeats vs mutates;
- **complexity profile** — a vector (rhythmic / pitch / technical / harmonic /
  playability / structural), orthogonal to length and period.

`phrase_length` is **not** a new control here — it is the output of S4 boundary
detection, consumed as context.

## Inputs / Outputs

- In: a target `TickRange` (or `bar_count`), a `StructureControl`, a
  `GenerationSeed`.
- Out: a `Score`/region whose material matches the control, plus
  `StructureMetrics` (provenance) describing what was actually produced.

## Approach

A **constraint compiler over the S6 generator** (the ComplementArranger pattern,
ADR-0012 / ADR-0015), not a new generation core:

1. Generate a `pattern_period`-length base motif via S6.
2. Tile it across the target span; vary copies by `variation_rate` while
   preserving identity by `repeatability`.
3. Check the loop seam (`loopability`).
4. Score the result with `StructureMetrics`; reject / rerank against the request.

Metrics are a self-contained analysis (self-similarity matrix / autocorrelation
of onset & contour features / motif recurrence / loop-seam), deterministic
(SPEC §6), living in the feature layer.

## Phases (per ADR-0015; "measure before target")

- **Phase 0 — metrics.** `StructureMetrics` over any span (no generation change).
- **Phase 1 — controls.** `StructureControl` + the tile/vary compiler over S6.
- **Phase 2 — scoring loop.** Reject / rerank candidates by metric distance
  (simple scoring + sort over a small candidate set — *not* DP).
- **Phase 3 — corpus.** Compute the same metrics on imported material; persist as
  `ChunkMeta` fields (schema bump) and, later, S7 node attributes.
- **Phase 4 — UI / edit-ops.** Basic/advanced/expert tiers; "make less
  repetitive", "double pattern length", "add variation every 2nd bar"
  (S11 region regeneration).

## Acceptance criteria

- Deterministic for a fixed seed + control (property test).
- `pattern_period = 1 bar`, `repeatability` high ⇒ the base bar recurs across the
  target span; detected pattern period ≈ requested.
- Complexity axes are independent: a short-period result can score high technical
  complexity; a long-period result can score low.
- Loopability check rejects a seam discontinuity on a defined test set.
- `StructureMetrics.detected_pattern_period` recovers the period of a synthetic
  tiled input.
- Structure-layer code depends on neither the graph layer nor DP/Viterbi.

## Non-goals (explicit)

- No graph layer, no DP/Viterbi here (ADR-0015 §7). Metrics are *designed to feed*
  S7 / DP later, not to consume them.
- No corpus-schema persistence until Phase 3.

## See also

- [`../adr/0015-structure-controls-and-metrics.md`](../adr/0015-structure-controls-and-metrics.md)
- [`../glossary.md`](../glossary.md) §7 (features/analysis), §8 (generation)
- [`S6-rule-generator-v0.md`](S6-rule-generator-v0.md),
  [`S4-phrase-boundary-detection.md`](S4-phrase-boundary-detection.md),
  [`S7-graph-layer.md`](S7-graph-layer.md),
  [`S11-region-regeneration.md`](S11-region-regeneration.md)
- [`../adr/0013-dp-viterbi-traversal.md`](../adr/0013-dp-viterbi-traversal.md)
  (the traversal that consumes these metrics — separate, later)
