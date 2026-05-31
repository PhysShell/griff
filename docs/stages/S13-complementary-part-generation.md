# S13: Complementary part generation (ComplementArranger)

Status: planned
Depends on: S6 (rule generator), ADR-0011 (canonical port of feature/generate)
ADRs: ADR-0012, ADR-0011

> Roadmap note: appended as the next free stage number (append-only, per the
> stage-label audit). Logically it sits between the single-part generator (S6)
> and the graph layer (S7) — S7 later learns complement relations from the
> corpus. See [`../audit/2026-05-s13-complementary-arranger.md`](../audit/2026-05-s13-complementary-arranger.md).

## Goal

Given an existing part A, generate a complementary part B that is related to A
on chosen musical axes and deliberately contrasting on others — arrangement, not
"another riff". Rule-based and deterministic.

## Inputs / Outputs

- In: a `Score` with at least one note-bearing `Track` (part A), a
  `ComplementSpec` (`RelationMode` + per-axis intent), a `GenerationSeed`.
- Out: the `Score` with an added `Track`/`Voice` (part B), or a
  `Vec<ComplementCandidate>` (one per mode/variant) carrying per-axis relation
  scores and provenance.

## Approach

ComplementArranger is a **constraint compiler over the S6 generator**, not a new
generator:

1. **Analyse A** into a part profile (rhythm/onset grid, accent pattern,
   register band, contour, normalised density, technique multiset, harmonic
   context). Requires the richer feature layer from ADR-0011.
2. **Pick a `RelationMode`** — one of `rhythm_lock`, `register_contrast`,
   `call_response`, `support_layer`, `octave_double`, `counter_melody`.
3. **Derive** a concrete S6 request: relative intent ("register = A − octave",
   "density = 0.6·A", "harmony = A", "technique ≠ A") compiled into absolute
   `GenerationConstraints` + `source_rhythms` + pitch material. `rhythm_lock`
   reuses `RhythmCopyPitchSubstitute` with A's onset grid as `source_rhythms`.
4. **Generate B** via the existing S6 generator and lift it into a `Track`/`Voice`
   on the shared `MasterBar`s of A's `Score`.
5. **Validate the pair**: playability per part (S6 filter) plus a
   harmonic-compatibility check over (A, B) — no dissonant clashes on coincident
   onsets, no register mud.

Generative-first: B is derived from A by rule; no corpus pair mining. `ChunkMeta`
/ corpus schema unchanged (`schema_version` = 1).

## Acceptance criteria

- Deterministic for a fixed seed and fixed A (property test).
- B respects the chosen `RelationMode`: e.g. `rhythm_lock` ⇒ B's onsets match
  A's onset grid; `register_contrast` ⇒ B's register band is disjoint from A's;
  `support_layer` ⇒ density(B) < density(A).
- The pair validator rejects coincident-onset dissonances and register mud on a
  defined test set.
- A `ComplementSpec` with `mode = octave_double` reproduces A's contour an octave
  away within the pitch range.
- P2 `complement_request` fuzz target (structure-aware, ADR-0010): arbitrary
  (score, spec, seed) inputs never panic; output shares A's `MasterBar`s and
  ticks_per_quarter; B's notes are in range; fixed seed stays deterministic.

## Open questions

- Harmonic-compatibility thresholds (allowed coincident intervals) — calibrate
  on the corpus.
- Default per-axis ratios (e.g. support-layer density factor) before S9 feedback
  exists to tune them.
- Where the part profile lives: extend the feature layer vs a dedicated
  `PartProfile` type in the canonical model.

## See also

- [`../glossary.md`](../glossary.md) §8 (Complementary part, Complement relation,
  Relation mode), §9 (Complement hyperedge), §10 (Relation preference)
- [`../adr/0012-complementary-part-generation.md`](../adr/0012-complementary-part-generation.md)
- [`../adr/0011-retire-legacy-linear-model.md`](../adr/0011-retire-legacy-linear-model.md)
- [`S6-rule-generator-v0.md`](S6-rule-generator-v0.md),
  [`S7-graph-layer.md`](S7-graph-layer.md)
- [`../fuzzing.md`](../fuzzing.md) (`complement_request`, P2)
