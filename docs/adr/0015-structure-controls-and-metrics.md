# ADR 0015: Separate structure controls and metrics from complexity

Date: 2026-06-02
Status: Proposed

## Context

Generation today is controlled by `GenerationConstraints` (bar count, meter,
tempo, PPQN, pitch range) plus a `strategy` and `source_rhythms`. There is no
first-class notion of *how the material is organised over time*: how long the
generated region is, how long the repeating idea inside it is, how strictly it
repeats, and how dense/technical it is. A single "complexity" knob conflates
all of these — and that is a real metric bug: a short repeated tapping ostinato
is rhythmically short-period but technically hard, while a long through-composed
line can be technically trivial. Length is not complexity; period is not
complexity; repetition is not complexity.

Much of the vocabulary already exists in the model and glossary: a *region* is a
[`TickRange`](../glossary.md) (selection / region regeneration, S11); *phrase
length* is the **output** of phrase-boundary detection (S4); `feature` and
`classify` already measure density, pitch range, and coarse style; `Motif` and
`Rhythm cell` are named. What is missing is (a) explicit controls for *target
span*, *pattern period*, *repeatability/variation*, and a *complexity profile*
(a vector, not a scalar), and (b) measured metrics for the same axes, computable
from any span.

This is a cross-cutting concern (it touches S6 generation, the feature layer,
the S7 graph layer, and S11 region regeneration), so it needs a recorded
direction before code. A specific question must be settled: does this require
the S7 graph layer or the DP/Viterbi traversal (ADR-0013)? It does not — and the
dependency in fact runs the other way.

## Decision

We add a **structure model** with three roles kept strictly separate (the same
spec / fact / provenance split as ADR-0012's complement relation):

1. **`StructureControl` (input)** — what the user asks for: `target_len_ticks`,
   `pattern_period_ticks: Option<…>` (`None` = through-composed), `repeatability`,
   `variation_rate`, `loopability`, and a `ComplexityProfile`
   (`rhythmic / pitch / technical / harmonic / playability / structural`, each a
   normalised scalar). Complexity is an **orthogonal vector**, never folded into
   length or period.

2. **`StructureMetrics` (output / provenance)** — what a produced candidate
   actually is: detected pattern period, repeatability score, variation score,
   loopability score, structural complexity. Used to reject / rerank candidates
   against the requested `StructureControl`.

3. **Per-segment structural features (corpus / graph)** — the same metrics
   computed over real imported material, to be stored as node attributes when
   the graph layer exists.

Further decisions:

4. **The structure layer is a constraint compiler over the S6 generator, not a
   new generator** (the ComplementArranger pattern, ADR-0012). It generates a
   `pattern_period`-length motif, tiles it across `target_len_ticks`, varies the
   copies by `variation_rate` while preserving identity by `repeatability`, and
   checks the loop seam. S6 stays a primitive; sub-bar periods are handled by the
   compiler slicing the target, not by changing S6.

5. **`phrase_length` is not a new control.** It is the output of S4 phrase-boundary
   detection; the structure layer consumes it rather than duplicating it.

6. **Metrics are a self-contained analysis** (self-similarity matrix /
   autocorrelation of onset & contour features / motif recurrence / compression
   ratio / loop-seam), living in the feature layer. They are deterministic
   (SPEC §6) and depend on nothing below them.

7. **The structure layer does NOT depend on the S7 graph layer or DP/Viterbi
   (ADR-0013).** Generation is local tiling+variation over one span; candidate
   selection is scoring + sort over a small candidate set, not a sequence
   optimisation. Corpus retrieval by structural behaviour is node-attribute
   filtering, not hyperedge traversal. The dependency runs the other way:
   `StructureMetrics` are designed to **become** S7 node attributes and DP
   transition-cost features later. They are a *supplier* to the graph/DP, never a
   *consumer*. (A small **local** DP could later infer `pattern_period` by optimal
   segmentation, but autocorrelation suffices and this is distinct from the S7
   graph-traversal Viterbi — deferred.)

8. **Order: measure before target.** The measurement metrics (a) land before
   treating complexity as a budget the generator must hit (b), so candidates can
   be scored before they are constrained.

9. **Naming.** Internal: `pattern_period_ticks`, `repeatability`,
   `variation_rate`, `loopability`, `ComplexityProfile`. UI: tiered basic /
   advanced / expert controls (`Short loop` / `1-bar riff` / … rather than raw
   tick counts). CLI: `--pattern-period`, `--repeatability`, `--variation`,
   `--complexity`.

10. **Roadmap.** Delivered as a new appended stage **S14** (append-only, per the
    stage-label history in `docs/audit/`), depending on S6. Logically it sits
    beside S6/S11 and feeds S7; appended rather than renumbering S7…S13.

## Consequences

- griff gains controllable structure: "4 bars, 1-bar base pattern, high
  repetition, light variation, simple" instead of "generate a riff".
- The "complexity = length" bug is designed out: complexity is a separate
  vector from period/length/repetition.
- The S7 graph layer and DP/Viterbi get a clean, pre-computed signal
  (structural behaviour per segment) when they arrive — retrieval "by type of
  musical behaviour", not just by notes — without S14 depending on them.
- Composes with ComplementArranger: a complement part can carry its own
  `StructureControl`; the axes are orthogonal.
- Accepted: a corpus-schema bump (`ChunkMeta` gains structure fields,
  `schema_version` += 1) is needed when per-segment metrics are persisted
  (Phase 3); deferred until then.
- Accepted: risk of control-surface explosion; mitigated by the basic/advanced/
  expert UI tiering and a single internal `ComplexityProfile`.
- Accepted: faithful pattern-period inference on messy real material is
  non-trivial; the first metric pass uses autocorrelation / self-similarity and
  may be refined later.
