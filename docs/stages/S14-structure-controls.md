# S14: Structure controls and metrics

Status: in progress — Phase 3 (corpus persistence) landed (2026-06-10)
Depends on: S6 (rule generator), S4 (phrase boundaries, for `phrase_length`)
ADRs: ADR-0015

> Progress: Phase 0 ships the `structure` module — `measure_structure` →
> `StructureMetrics` (detected pattern period in bars + ticks, repeatability,
> variation, loopability, structural complexity) via per-bar self-similarity
> autocorrelation (contour-aware since 2026-06-09), plus the P2
> `structure_metrics` fuzz target. Phase 1 adds `StructureControl` +
> `generate_structured` — the tile/vary constraint compiler over S6:
> `pattern_period`-length base via S6, tiling across the target span,
> seed-deterministic variation (repeatability gates verbatim copies,
> variation_rate gates per-bar rhythm-preserving transposition), measured
> metrics returned as provenance; `None` period = through-composed S6
> passthrough. Phase 2 adds the scoring loop: `structure_axes` (period /
> repeatability / variation agreement between control and measured metrics,
> ADR-0017 facts), the uniform `structure` v1 `WeightPolicy`,
> `StructuredCandidate::scored` (explainable `Scored` envelope with seed +
> policy provenance), `rank_structured` (fixed tie-break), and
> `generate_structured_set` (deterministic candidate set over derived seeds).
> Phase 3 persists the same metrics on imported material: `StructureSnapshot`
> (the corpus form of `StructureMetrics`) on `ChunkMeta`, schema v2 with v1
> backward compatibility, measured by `griff curate` at curation time.
> Pure, deterministic, and independent of the graph layer / DP.
> Remaining: sub-bar (beat-level) period detection, the full per-axis
> `ComplexityProfile`, a P2 `structured_request` fuzz target (deferred: no
> nightly toolchain in the landing environment), then Phase 4 below.

> Roadmap note: appended as the next free stage number (append-only, per the
> stage-label history in [`../audit/`](../audit/)). Logically it sits beside the
> single-part generator (S6) and region regeneration (S11) and *feeds* the graph
> layer (S7); it deliberately does **not** depend on S7 or DP/Viterbi (ADR-0013).

## Known limitations (Phase 0 — deferred refinements)

Documented now, to be addressed in later increments (ADR-0015 framed Phase 0 as
a first pass). The metrics are intended to become user-tunable, so an honest
exact-pitch baseline is acceptable for the first cut.

- [x] **Transposed repeats.** *Fixed in the metrics.* Bar similarity is now
      contour-aware (`structure::bar_similarity`): a weighted onset-grid
      (rhythm) + pitch comparison, where a transposed repeat — identical
      rhythm, constant non-zero interval shift — earns partial pitch credit.
      `A A' A''` reads as a 1-bar period with medium repeatability, rhythm-only
      tiles as partial, unrelated material as low (decisions.log 2026-06-09).
      This is the motif-identity measure the Phase-1 tile/vary compiler will
      grade its variations against.
- [x] **Trailing empty bars.** *Fixed at the importer.* `build_master_bars` now
      loops `while bar_start < end_tick || master_bars.is_empty()`, so content
      ending exactly on a barline no longer appends a sentinel bar that lowered
      `loopability_score` and diluted period/repeatability. Re-blessed the
      import / inspect / classify / roundtrip / characterize goldens
      (decisions.log 2026-06-03).
- [ ] Sub-bar (beat-level) period detection and the full per-axis
      `ComplexityProfile`.

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

- **Phase 0 — metrics.** ✅ `StructureMetrics` over a score track (no generation
  change). Bar-resolution period detection; sub-bar cells + full complexity
  vector deferred.
- **Phase 1 — controls.** ✅ `StructureControl` + the tile/vary compiler over S6
  (`generate_structured`); loopability targets + `ComplexityProfile` controls
  deferred to the Phase-2 scoring loop and the complexity increment.
- **Phase 2 — scoring loop.** ✅ Reject / rerank candidates by metric distance
  (simple scoring + sort over a small candidate set — *not* DP): generate a
  set, rank by control↔metrics agreement under the shared `Scored` vocabulary;
  rejection is the caller's threshold cut on the aggregate. A loopability
  agreement axis joins when the control grows a loopability target.
- **Phase 3 — corpus.** ✅ Compute the same metrics on imported material; persist
  as `ChunkMeta.structure` (`StructureSnapshot`, schema v2; v1 records parse as
  `None`), measured by `griff curate` on the first note-bearing track. The
  "S7 node attributes" half lands with the graph layer.
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
- ~~No corpus-schema persistence until Phase 3.~~ (Phase 3 landed: schema v2.)

## See also

- [`../adr/0015-structure-controls-and-metrics.md`](../adr/0015-structure-controls-and-metrics.md)
- [`../glossary.md`](../glossary.md) §7 (features/analysis), §8 (generation)
- [`S6-rule-generator-v0.md`](S6-rule-generator-v0.md),
  [`S4-phrase-boundary-detection.md`](S4-phrase-boundary-detection.md),
  [`S7-graph-layer.md`](S7-graph-layer.md),
  [`S11-region-regeneration.md`](S11-region-regeneration.md)
- [`../adr/0013-dp-viterbi-traversal.md`](../adr/0013-dp-viterbi-traversal.md)
  (the traversal that consumes these metrics — separate, later)
