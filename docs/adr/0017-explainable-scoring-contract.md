# ADR 0017: Unify scoring into axes, weights, rationale, and a derived aggregate

Date: 2026-06-03
Status: Proposed

## Context

The same shape — *score something, keep per-axis detail, explain why, expose a
tunable weighting* — recurs across the canon, described five times in four
incompatible forms. Two already live in code:

- **Phrase-boundary detection** (S4, `core/src/boundary.rs`): a `PhraseBoundary`
  carries an aggregate `score: f64` in `[0,1]`, a `BoundaryReason` (the per-signal
  booleans — a rationale), and the weighting lives as data in
  `BoundaryConfig.weights: [f64; 6]` plus a `threshold`.
- **ComplementArranger** (S13, `core/src/complement.rs`): a `ComplementCandidate`
  carries `AxisScores` (four named `f64` axes), a `mode`, a `seed`, and a separate
  `PairValidation`. Here the weighting is *implicit* — there is no weight vector.

Three more are on paper, each minting its own shape:

- the **DP cost function** (ADR-0013) — a named-term signed sum, with weights
  exposed as data and ties broken by a fixed rule, explicitly the surface S9
  tunes;
- **`ComplexityProfile` / `StructureMetrics`** (ADR-0015) — deliberately a
  *vector, never a scalar*, in a measured-provenance role;
- the glossary **"Quality score"** (§8) — still a flat scalar list
  (similarity / novelty / playability / …), and S9's **"Relation preference"**
  (§10), which wants to learn weights over axes.

Each reinvents the shape: some carry a rationale, some don't; some split weights
as data (boundary, ADR-0013), one has no weights at all (complement), one insists
on a vector (ADR-0015), one stays a flat scalar ("Quality score"). They cannot
share a tuning surface (S9 would tune a different thing per feature), a UI (no
single "why this candidate" inspector), or a vocabulary. Implementing 0013 / 0015
each with its own bespoke scoring type pays a migration tax later — and the
legacy-linear-model retirement (ADR-0011) already showed how that migration
feels on a smaller codebase.

Two hazards are concrete and worth settling before the paper consumers harden:

- **Determinism under feedback.** SPEC §6 fixes "deterministic under a fixed
  seed". But once S9 *mutates weights*, a score and the candidate ordering it
  induces are only reproducible relative to a *weights version*; a seed alone no
  longer pins them. ADR-0013 §3 already had to reach for a fixed tie-break rule;
  that need is general.
- **A naming collision.** "Evidence" already means *import-side* provenance — why
  an articulation is believed (`MIDI evidence`, `technique evidence`, glossary
  §3 / §5). A scoring "why" must not reuse that word.

This is a contract, not a feature, so it is recorded as an ADR before code. It
ratifies §2.1–§2.2 of the design synthesis
[`docs/audit/2026-06-expressive-control-and-scoring.md`](../audit/2026-06-expressive-control-and-scoring.md);
the presets / affect / style-region and idiom-axis work from the same note
(§2.3–§2.5) is deliberately left to follow-up ADRs that build on this shape.

## Decision

We fix **one scoring vocabulary** — four parts and three roles, mirroring the
spec / fact / provenance split of ADR-0012 and ADR-0015.

1. **Four parts.** A score is always: **axes** (named, normalised per-axis
   measurements, represented as labelled *data* — not a fixed struct-of-the-day);
   **weights** (a *separate*, data-valued policy vector — the surface S9 tunes,
   never hardcoded into the code that computes axes); a **rationale** (the
   explainable trace: which axes fired / contributed, thresholds crossed — the
   generalisation of `BoundaryReason`); and a derived **aggregate** (the scalar =
   `aggregate(axes, weights)`). The aggregate is *derived*, never the source of
   truth.

2. **Anti-scalar, as canon** (lifts ADR-0015's local stance to a rule). Axes are
   the truth; the aggregate is a convenience for sorting and UI. No score may be
   stored as a bare scalar with its axes discarded. This reframes glossary §8
   **"Quality score"**: it is the *aggregate of a quality axis-vector*
   (similarity / novelty / playability / density / preference / style fit), not a
   primitive.

3. **Axes-as-data / weights-as-policy is mandatory** (generalises ADR-0013 §4).
   A scorer computes axes from features and exposes the weight vector as data — a
   *named, versioned weight policy* — so S9 tunes weights without touching the
   axis computation. `aggregate` is a pure function of `(axes, weights)`.

4. **Axes must be measurable** (the falsifiability gate). An axis is a
   deterministic function of material — `syncopation`, `register_overlap`,
   `loop-seam` are axes; `"nervous"`, `"bright"`, `"swancore"` are **not**. An
   unmeasurable descriptor is a *preset* (a named bundle of target ranges +
   weights) **over** axes, never an axis itself. Presets, affect, and style
   regions are deferred to their own ADR; this one only fixes the axis/weight/
   rationale/aggregate shape they will reuse.

5. **Hard gates and soft axes stay distinct, never merged.** Validity checks that
   *reject* a candidate — the playability filter, the `PairValidation` gate
   (`is_clean`) — are **not** axes and do not enter the weighted aggregate. Axes
   *rank* what survives the gates. Folding a hard gate into a soft weight (so a
   playable-but-dull candidate can outscore an unplayable-but-pretty one) is
   forbidden; the gate runs first, the aggregate ranks the survivors.

6. **One output shape, `Scored<T>`** — the produced value plus its axes,
   rationale, and provenance — replaces today's ad-hoc `PhraseBoundary { score,
   reason }` and `ComplementCandidate { …, axis_scores }`. Provenance carries
   exactly what makes the aggregate reproducible: the **seed** (where RNG
   applies) *and the **weights version*** it was scored under.

7. **Determinism under feedback** (generalises ADR-0013 §3, extends SPEC §6). An
   aggregate, and the candidate ordering it induces, are reproducible only
   relative to `(seed, weights-version)`; ties break by a fixed, documented rule
   (e.g. lowest candidate index), making the ordering total and stable. A stored
   score without its weights-version is not reproducible and is forbidden.

8. **Naming, resolving the collision.** **evidence** stays import-side (why an
   articulation is believed — MIDI / Guitar Pro provenance, glossary §3 / §5).
   Scoring uses **rationale** for the explainable "why this score" and
   **provenance** for output metadata (mode, seed, weights-version). Under this
   vocabulary: complement `AxisScores` and boundary `score` + `BoundaryReason` +
   `weights` are instances of axes / aggregate / rationale / weights;
   `ComplexityProfile` and `StructureMetrics` (ADR-0015) are axis-vectors; the DP
   cost terms (ADR-0013) are a signed axis-vector whose weights are the same data
   S9 tunes.

9. **Scope: contract only — no new scorer, no behaviour change.** This ADR adds
   vocabulary and a shape, not a capability. The first code slice is purely
   additive: generalise the existing complement `AxisScores` / `ComplementCandidate`
   and boundary `score` / `BoundaryReason` / `weights` onto the shared `Scored` /
   axes / weights / rationale vocabulary, behind characterization tests, with no
   golden changes. The DP cost function (ADR-0013), `StructureMetrics`
   (ADR-0015), and the S9 tuning surface adopt the shape when they land; none is
   built here.

10. **Direction of dependency** (as ADR-0015). Scoring is a *supplier*: it
    defines the shape S9 tunes and S7 / DP consume. It depends on neither. A
    scorer computes axes from features alone and is deterministic (SPEC §6).

11. **Roadmap: no new stage.** This is cross-cutting canon (it touches S4
    boundary, S6 / S13 generation, S7 DP, S9 feedback, S14 structure). It is a
    contract recorded now so those stages converge on one shape instead of each
    minting a scoring type — the same way ADR-0016 is a cross-frontend contract
    without its own stage.

## Consequences

- One vocabulary: boundary, complement, structure, DP cost, and quality scores
  share axes / weights / rationale / aggregate. One "why this candidate"
  inspector serves every surface, and S9 gets a *single* tuning surface (the
  weight vector) instead of one per feature.
- The anti-scalar rule becomes canon, not just ADR-0015's local position;
  "Quality score" stops being a flat scalar.
- Determinism is defined under feedback: a score is reproducible relative to
  `(seed, weights-version)`, closing the gap SPEC §6 leaves once weights move.
- The evidence / rationale collision is pre-empted before either name lands in
  scoring code.
- Hard gates (reject) stay separate from soft axes (rank), so a validity failure
  can never be out-weighted by a pretty score; and the measurable-axis rule keeps
  affect / style honest — they must decompose into axes before they can be
  scored, which is what makes a later affect/preset ADR falsifiable.
- Cost: the two existing bespoke shapes (`PhraseBoundary`,
  `ComplementCandidate` / `AxisScores`) must migrate to `Scored` under
  characterization tests — a real refactor, though additive and golden-neutral.
- Accepted: every stored score now carries a weights-version; persistence
  (corpus, S9 history) gains a field when it lands — a future `schema_version`
  bump, like ADR-0015's deferred one.
- Accepted: this is a paper contract, only proven when the *second* consumer (DP
  cost or S9) actually reuses the shape. The first migration (complement +
  boundary) must reshape to the shared vocabulary and nothing else; anything it
  cannot express is a signal to extend the contract, not to fork it (echoing
  ADR-0016's "proven by the second renderer").
