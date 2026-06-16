# ADR 0023: Control pitch/contour spread of complementary parts

Date: 2026-06-16
Status: Proposed

## Context

In the grid-locked complement modes (`rhythm_lock`, `register_contrast`,
`call_response`) part B's pitch is chosen by hashing each onset's position into
the band's scale ladder (`pitch_index(seed, i, ladder.len())`). After ADR-0012
and the bottom-octave fix the ladder spans A's whole register band, but the
choice is *all-or-nothing*: a uniform draw over the entire band, with no control
over **how far B wanders** between a static line and a register-wide one. Users
asking for a "complementary guitar" want exactly that dial — "how much does B
depart from a static line / A's contour".

Three constraints frame the decision:

- **Determinism is a hard rule.** SPEC §6: *generation is deterministic under a
  fixed seed*; the glossary defines a deterministic generator as
  `same seed/input → same result`. Any knob must change the deterministic output,
  never introduce nondeterminism.
- **S6 already owns "controlled variability".** Its strategies (constrained
  random walk with leap/repeat penalties, motif-transpose + variation) are seeded
  and style-bounded (density within corpus mean ± 1σ). Variation is not foreign to
  the generator; it is the generator's native idea, exposed as a parameter.
- **Grid-locked means onset-locked.** The grid modes deliberately do **not**
  route through an S6 round-trip — "A's onsets already respect A's timeline;
  regeneration could only misalign" (`decisions.log`). `rhythm_lock`'s acceptance
  criterion is `B's onsets match A's onset grid` and `rhythm_similarity == 1.0`.
  A variability knob that drifts B *off the grid* would break the mode's contract.

The repository already has the pattern for such a knob: the spec / fact /
provenance split of ADR-0012, carried by `GestureControl` / `StructureControl`
(ADR-0015) — a typed *ask* compiled over the S6 generator, run deterministically,
returning the produced *is* as provenance.

## Decision

We add **`VariationControl`** — the pitch-variability *ask*, orthogonal to
`RelationMode`, a sibling of `GestureControl` and `StructureControl`.

1. **One axis to start: `pitch_spread ∈ [0.0, 1.0]`.** The fraction of the band's
   scale ladder B may use. `0.0` pins every note to the band's anchor degree (a
   static line, still locked to A's grid); `1.0` uses the whole band — the
   unconstrained default, i.e. exactly today's `arrange_complement`.

2. **Pitch only, never onsets.** The knob narrows the modulo of the existing
   seeded pitch hash; it does not move a single onset. Grid-locked modes stay
   grid-locked — `rhythm_similarity` stays `1.0`. It applies to the
   ladder-substitution modes (`rhythm_lock`, `register_contrast`,
   `call_response`). `octave_double` and `support_layer` have no pitch
   degree-of-freedom to spread; `counter_melody`'s variability is S6's own
   (mapping `pitch_spread` onto the walk's leap penalty is a future axis).

3. **A separate entry point, not a `ComplementSpec` change.**
   `arrange_complement_varied(score, idx, spec, seed, control) -> VariedComplement`.
   `ComplementSpec` has many construction sites; extending it would break every
   caller for a concern that composes cleanly on top — the `generate_gestured`
   precedent (a separate entry, not an extra request field). `arrange_complement`
   stays and is exactly `arrange_complement_varied(.., VariationControl::FULL)`.

4. **Deterministic (SPEC §6).** The window only shrinks the range of the seeded
   hash; the same `(score, idx, spec, seed, control)` always yields the same B.
   No new RNG is introduced.

5. **Provenance is ask-vs-is.** `VariedComplement { complement, control,
   realized_spread }` carries the control that asked and B's realized pitch
   ambitus as a fraction of the target band — the `GesturedCandidate` duality.
   An out-of-range control is the typed `VariationError::InvalidControl`, never a
   silent clamp.

6. **Future axes live on the same struct.** Contour adherence (track A's
   up/down motion), a leap budget, and the `counter_melody` walk-penalty mapping
   are added as further `VariationControl` fields. **Onset drift is explicitly
   not a `VariationControl` axis** — rhythmic independence already has a home in
   `counter_melody` (and any future blended mode), not in a knob bolted onto a
   grid-locked mode.

## Consequences

- griff gains a "how static ↔ how wandering" dial for complement pitch, without
  violating determinism or any mode's grid-lock contract.
- The default path is **byte-identical** (`pitch_spread = 1.0` is the identity
  window), so the corpus schema, goldens, and CLI snapshots are unaffected; the
  feature is purely additive.
- Composes with `GestureControl` / `StructureControl` — the axes are orthogonal,
  as ADR-0015 anticipated ("a complement part can carry its own control").
- Accepted: `realized_spread` is a coarse ambitus-over-band ratio, not a
  per-degree histogram; the window anchors at the band floor (small spread hugs
  the low register) rather than centring on A's contour — both are refined by the
  future axes above.
- Accepted: `counter_melody` is not wired to the knob in this increment; its
  variability stays S6's until the walk-penalty mapping lands.
- A CLI `--variation <0..1>` surface is the natural follow-up; it defaults to
  `1.0`, so existing snapshots continue to hold.
