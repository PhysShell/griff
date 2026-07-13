# S15: Tonal context and harmonic control

Status: in progress — Phase 0 (evidence audit) and Phase 1 (shared tonal core)
accepted and closed on 2026-07-12; Phase 2 is next
Depends on: S1 (canonical score), S5 (corpus), S6 (rule generator)
Builds on: S13 harmonic-context analysis
Feeds: S6 generation, S7 graph costs, S11 regeneration, S13 complement

## Goal

Make tonal and harmonic context explicit, uncertain, scoped, and reusable before
it is allowed to influence generation. Separate observed evidence from inferred
musical meaning; preserve an honest abstention path; never turn one best guess
into a hard seven-note whitelist.

S15 owns the meaning of tonal/harmonic states. S7 owns global path optimisation
over such states. S8 displays the alternatives and provenance. S9 learns from
human choices among them.

## Guardrails

- A real-song key result is a tonal hypothesis for one `EvidenceScope`, not
  verified ground truth.
- Confidence thresholds and automatic scope selection require calibration; a
  larger margin alone does not prove that one track is the correct reference.
- The observed `PitchClassSet` remains distinct from an inferred scale or tonal
  hierarchy. Chromatic passing tones, borrowed notes, and tensions are not
  automatically errors.
- `None` / ambiguous context is a valid result. No silent fallback to C major,
  the lowest pitch, or the highest-margin track.
- Cadence and generation integration remain frozen until their phase-specific
  acceptance gates are met.

## Phase 0 — evidence audit and diagnostics ✅

Accepted and closed.

- Audited the existing generation input and established that
  `PitchMaterial.root` was an anchor derived from the minimum input pitch, not a
  tonic.
- Measured `WholeScore`, `Track`, and `Voice` evidence on real and synthetic
  inputs.
- Established raw evidence vocabulary: onset counts, duration mass, note count,
  and observed pitch range.
- Demonstrated that scope can change the winning tonal hypothesis (including the
  Wolf & Bear whole-score vs track conflict).
- Rejected confidence cut-offs inferred from the small diagnostic fixture set.

Primary record:
[`../audit/2026-07-tonal-context-phase0.md`](../audit/2026-07-tonal-context-phase0.md).

## Phase 1 — shared evidence/inference core ✅

Accepted and closed.

`core/src/tonal.rs` now provides:

- `EvidenceScope::{WholeScore, Track, Voice}`;
- `PitchEvidence::measure` with raw integer `onset_counts`, `duration_mass`,
  `note_count`, and observed `feature::PitchRange`;
- `estimate_key` returning all 24 major/natural-minor `TonalCandidate`s,
  best-first, with per-candidate correlation and `scale_fit`;
- `TonalEstimate::confidence_margin` as winner minus runner-up;
- duration-only KS v1, with onset-count fallback only when total duration mass is
  zero.

`complement::estimate_harmony` delegates to the shared inference core and keeps
its public winner-only projection. Focused validation proved:

- `HarmonicContext`: 16/16 exact, 0 changed;
- structure consumer: 7/7 byte-identical;
- evidence mapping: 39/39, 0 mismatches;
- histogram additivity: pass;
- 24 finite candidates per non-empty scope: pass;
- generation smoke: 30/30 byte-identical.

Cloud implementation: `6f9114d` (red), `184b586` (green), `e2c9c7f` (docs),
`af26206` (accepted/closed). Local validation: `bd2c7c8`; archival:
`3993bb0`.

## Phase 2 — explicit scoped context contract (next)

Allow generation-facing requests and provenance to carry an optional, explicit
scoped tonal estimate **without changing note selection yet**.

The exact type is a design output, not pre-decided, but it must preserve:

- the chosen `EvidenceScope`;
- the ranked estimate or an intentionally compact immutable projection;
- absence / ambiguity;
- deterministic serialisation and replay;
- provenance identifying how the estimate was measured.

Acceptance:

- requests without tonal context remain byte-identical to the Phase-1 baseline;
- context is optional and scope is explicit;
- no automatic whole-score/track/voice selection;
- no pitch restriction, reranker-weight change, cadence, or production behaviour
  change;
- round-trip and deterministic replay tests cover the new contract.

## Phase 3 — scope policy and confidence calibration

### Phase 3A — scope-selection experiments

Compare explicit policies rather than silently choosing the largest margin:

- selected/reference track;
- whole score;
- guitar-only subsets;
- weighted combinations of tracks;
- multiple competing scope estimates carried together.

A repetitive bass pedal or ostinato may yield a strong margin while describing
only one layer, so `argmax(margin)` is not an approved policy.

### Phase 3B — confidence calibration and synthetic controls

Build labelled, programmatic controls covering:

- exactly flat chromatic material;
- diatonic and pentatonic material;
- pedal tones and omitted tones;
- borrowed notes and secondary dominants;
- modal ambiguity;
- modulations / tonicisations;
- transpositions and alternative textures.

Report error and abstention behaviour by scope/material class. Only then may a
stable confidence vocabulary or threshold be proposed.

## Phase 4 — harmonic fixture DSL

Introduce a small external fixture language inspired by RomanText / harmonic
analysis DSLs, for example:

```text
C: I | vi | IV | V
a: i | VI | III | VII
C: I | V/V | V | I
```

The text format is for fixtures, debugging, and synthetic corpus generation.
Core uses typed structures (`Degree`, `HarmonicFunction`, chord quality,
inversion, modulation/tonicisation); parser strings are not the domain model.

Acceptance:

- scripts transpose deterministically;
- scripts generate labelled symbolic fixtures with multiple textures;
- parser failures are typed and localised;
- no runtime dependency on Python, TensorFlow, MusicXML, or an external analysis
  service.

## Phase 5 — soft harmonic generation policy

Allow calibrated tonal/harmonic information to influence candidate generation or
ranking as a **soft preference**:

- observed pitch classes stay legal unless an explicit user constraint says
  otherwise;
- inferred hierarchy may reward chord/scale tones and controlled resolutions;
- ambiguous estimates abstain;
- borrowed/chromatic colour tones remain representable;
- A/B evaluation covers harmonic fit, closure, novelty, rhythm, register, and
  playability together.

No hard `inferred scale == allowed notes` shortcut.

## Phase 6 — local context and cadence

Move the remaining S6 cadence-aware-ending backlog here. Cadence requires local
section context, phrase boundaries, calibrated confidence, and an abstention
path; a global winner plus `last_note = tonic` is not a cadence model.

Candidate ending states and explainable resolution costs may be optimised through
the S7 layered-path engine once both stages provide stable contracts.

Acceptance:

- section-local context beats global-only context on defined fixtures;
- ambiguous/modulating regions may decline to force a cadence;
- endings are returned as ranked alternatives with explanations;
- context-free generation remains unchanged.

## Research inputs

See
[`../audit/2026-07-symbolic-harmony-and-evolution-research.md`](../audit/2026-07-symbolic-harmony-and-evolution-research.md).
The main inputs are:

- `ekzhang/harmony` and `napulen/romanyh`: layered DP, transition costs, and
  k-best global alternatives (algorithmic shape shared with S7);
- `napulen/AugmentedNet`: decomposed harmonic targets and synthetic labelled
  examples, not a runtime dependency;
- `napulen/harmalysis` / RomanText: fixture-language inspiration.

## Non-goals

- No classical SATB rules copied wholesale into swancore guitar generation.
- No neural runtime dependency in S15; neural assistance remains S12.
- No audio chroma/HMM round-trip while the source is already symbolic.
- No generic `MusicDPGodObject`; S7 owns a small path engine with separate
  state/cost clients.

## See also

- [`S6-rule-generator-v0.md`](S6-rule-generator-v0.md)
- [`S7-graph-layer.md`](S7-graph-layer.md)
- [`S8-preview-app.md`](S8-preview-app.md)
- [`S9-feedback-layer.md`](S9-feedback-layer.md)
- [`S13-complementary-part-generation.md`](S13-complementary-part-generation.md)
- [`../audit/2026-07-tonal-context-phase0.md`](../audit/2026-07-tonal-context-phase0.md)
