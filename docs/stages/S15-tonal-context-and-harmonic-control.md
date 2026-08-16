# S15: Tonal context and harmonic control

Status: in progress — Phase 0 (evidence audit) and Phase 1 (shared tonal core)
accepted and closed on 2026-07-12; Phase 2 (explicit scoped context contract)
implemented on 2026-08-16 and **pending acceptance**; Phase 3 follows only after
Phase 2 is accepted
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

## Phase 2 — explicit scoped context contract (implemented, pending acceptance)

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

### What landed

`core/src/tonal.rs` gained the carriable third layer:

- `TonalContext { scope, projection, provenance }`, built only through
  `TonalContext::measure(score, scope)` or `TonalContext::from_evidence`. Both
  take the scope from the caller, and **there is no scope-free constructor** —
  the core cannot pick a track, a voice, or the whole score on anyone's behalf.
- `TonalProjection { winner, runner_up, confidence_margin }` — the compact
  immutable form. Compact is safe because the ranked estimate is *replayable*
  from the carried scope (`PitchEvidence::measure` + `estimate_key`); what it
  must not drop is the uncertainty, so the runner-up and the margin stay. No
  threshold and no `is_confident` — that is Phase 3B.
- `TonalProvenance { method, weighting, note_count }`, with
  `TonalMethod::KsV1` and `TonalWeighting::{DurationMass, OnsetCounts}`. The
  KS v1 duration/onset fallback was previously invisible in the result; the
  context now names the histogram that actually weighted the estimate.
  `resolve_weights` reports the branch it took and the new
  `estimate_with_weighting` is the single place that decides it, so the
  estimate and its provenance cannot drift apart. `TonalEstimate`'s public
  shape is unchanged.
- Two absences, never conflated: no context at all (`Option` on the request)
  versus a context whose scope had nothing to estimate (`projection: None`).
  The projection and the weighting are derived from one `Option`, so their
  absence is coherent by construction rather than by two matching conditions.
- Serde across the tonal types with `deny_unknown_fields`, and a tagged
  `EvidenceScope` (`{"kind": "track", "at": 1}`) so a scope read back out of an
  artifact is unambiguous.

`core/src/generation_input.rs` gained `GenerationAsk.tonal` and
`RankedSet.tonal`; `ranked_candidates` echoes the ask's context into the set
verbatim and reads it nowhere. Every existing caller opts out explicitly
(`tonal: None`) — the CLI has no scope flag, the cockpit's Generate panel no
scope control, and a Swang `generate` verb no scope word, so none of them may
have a scope measured on their behalf.

### Evidence

- `cargo test --workspace`: 1409 passed, 0 failed (1390 before; +19 in the new
  `core/tests/tonal_context.rs`).
- `cargo clippy --all-targets -- -D warnings`, `cargo fmt --all --check`: clean.
  `cargo doc --no-deps --workspace`: no new warning.
- Byte-identity: the new suite generates the same source twice, once with a
  context and once without, and compares candidates, derived seeds, aggregate
  bits, notes, the pitch palette, and the rerank policy id/version. The S6
  chain baseline golden (`core/tests/s6_chain_baseline.rs`, recorded before
  this field existed) is untouched and still passes.
- Ambiguity and absence: an exactly flat chromatic scope projects a zero margin
  with its rival intact; a silent scope abstains with `weighting: None`.
- Scope authority: a score whose two tracks sit a tritone apart reports C major
  or F# major strictly according to the scope asked for.
- Round-trip and replay: every scope shape round-trips exactly (floats
  included), the abstaining envelope is pinned byte-for-byte, a foreign field is
  refused at each level, and a context deserialised from JSON re-measures from
  its own carried scope to an identical value.

Implementation: `5d667ac` (red), `af97684` (green). Acceptance is a separate
gate and has not been given.

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
