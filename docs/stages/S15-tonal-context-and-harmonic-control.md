# S15: Tonal context and harmonic control

Status: in progress — Phase 0 (evidence audit) and Phase 1 (shared tonal core)
accepted and closed on 2026-07-12; Phase 2 (explicit scoped context contract)
accepted and closed on 2026-08-16 at `8799b55`; Phase 3 is next, and opens as
its own scope
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

## Phase 2 — explicit scoped context contract ✅

Accepted and closed at `8799b55` on 2026-08-16, after three independent review
rounds. **Frozen consequences:**

- the context carries its `PitchEvidence`, so replay is self-contained — that,
  and not the caller still holding the score, is what licenses the compact
  projection;
- `PitchEvidence::validate` is the single gate on the root of trust, enforced on
  both doors (the wire boundary and `TonalContext::from_evidence`);
- the artifact re-derives itself from that evidence and compares exactly, and
  every failure is a typed `TonalArtifactError`;
- the scope stays caller-owned: no scope-free constructor, no automatic
  whole-score/track/voice selection;
- Phase 2 is **carriage only** — no note selection, pitch restriction, rerank
  weight, or cadence reads the context, and generation stays byte-identical
  with and without one;
- changing the estimator means a new `TonalMethod` variant with its own
  re-derivation path, never an in-place edit of `KsV1`;
- the `u32::MAX`-onsets-per-pitch-class bound on `measure`/`validate` agreement
  is a recorded Phase-1 saturation limit, not a fixed defect.

Acceptance closes Phase 2 and authorises nothing beyond it. Confidence
calibration, automatic scope selection, and any tonal influence on generation
are Phase 3 work and need their own acceptance.

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

- `TonalContext { evidence, projection, provenance }`, built only through
  `TonalContext::measure(score, scope)` or `TonalContext::from_evidence`. Both
  take the scope from the caller, and **there is no scope-free constructor** —
  the core cannot pick a track, a voice, or the whole score on anyone's behalf.
  Fields are private with getters (`scope()`, `evidence()`, `projection()`,
  `provenance()`), so Rust callers cannot assemble an envelope either.
- The context carries the **`PitchEvidence`**, not just the scope. That is what
  makes the ranking replayable from the artifact *alone*:
  `estimate_key(context.evidence())` returns all 24 candidates with no score in
  hand and no second measurement pass.
- `TonalProjection { winner, runner_up, confidence_margin }` — the compact
  immutable form, and compact *safely* for the reason above. What it must not
  drop is the uncertainty, so the runner-up and the margin stay. No threshold
  and no `is_confident` — that is Phase 3B.
- `TonalProvenance { method, weighting }`, with `TonalMethod::KsV1` and
  `TonalWeighting::{DurationMass, OnsetCounts}`. The KS v1 duration/onset
  fallback was previously invisible in the result; the context now names the
  histogram that actually weighted the estimate. `resolve_weights` reports the
  branch it took and the new `estimate_with_weighting` is the single place that
  decides it, so the estimate and its provenance cannot drift apart.
  `TonalEstimate`'s public shape is unchanged, and `note_count` lives in the
  evidence rather than being stored a second time.
- Two absences, never conflated: no context at all (`Option` on the request)
  versus a context whose scope had nothing to estimate (`projection: None`).
- **The artifact proves itself.** There is one wire form (a private `Raw*`
  layer reached through `#[serde(into, try_from)]`), `deny_unknown_fields` at
  every level of it, and deserialisation that re-derives the whole context from
  the carried evidence and compares before returning the re-derived value.
  `TonalArtifactError` makes each failure a typed refusal. `EvidenceScope`
  serialises as one flat object (`{"kind": "track", "track": 1}`) because
  serde's tagged-enum representations accept foreign keys silently.
- **And the evidence proves itself too.** Re-derivation shows the estimate
  follows from the evidence; it says nothing about whether the evidence is
  possible, and the estimator reads the two histograms neither against each
  other nor against the span. So `PitchEvidence::validate` is the one gate on
  the root of trust, enforced on both doors — the wire boundary and the now
  fallible `TonalContext::from_evidence` — rejecting duration mass without
  onsets, mass beyond what its onsets could carry, a sounding class with no
  representative inside the observed span, span endpoints not covered *per
  class counting multiplicity* (C4..C5 puts both ends in class C and so needs
  two C onsets — testing mere presence would let one note stand at both ends of
  an octave), and totals that do not fit `u64`. `measure` stays infallible (its
  output is valid by construction) and both paths share one private `project`,
  so there is still exactly one projection rule. `resolve_weights` folds with
  `saturating_add` rather than `Iterator::sum`, because `estimate_key` is
  public over a struct with public fields and must not assume a summable
  histogram.

`core/src/generation_input.rs` gained `GenerationAsk.tonal` and
`RankedSet.tonal`; `ranked_candidates` echoes the ask's context into the set
verbatim and reads it nowhere. Every existing caller opts out explicitly
(`tonal: None`) — the CLI has no scope flag, the cockpit's Generate panel no
scope control, and a Swang `generate` verb no scope word, so none of them may
have a scope measured on their behalf.

### Evidence

- `cargo test --workspace`: 1434 passed, 0 failed (1390 before this phase's
  work; +44 in the new `core/tests/tonal_context.rs`).
- `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all
  --check`: clean. `cargo doc --no-deps --workspace`: no new warning.
  `cargo +1.92 check --workspace --all-targets` (MSRV): clean.
- Byte-identity: the suite generates the same source twice, once with a context
  and once without, and compares candidates, derived seeds, aggregate bits,
  notes, the pitch palette, and the rerank policy id/version. The S6 chain
  baseline golden (`core/tests/s6_chain_baseline.rs`, recorded before this field
  existed) is untouched and still passes.
- Ambiguity and absence: an exactly flat chromatic scope projects a zero margin
  with its rival intact; a silent scope abstains with `weighting: None`.
- Scope authority: a score whose two tracks sit a tritone apart reports C major
  or F# major strictly according to the scope asked for.
- Self-contained replay: one test builds the score inside a block, drops it, and
  recovers all 24 candidates from the JSON alone, checking the top two and the
  margin against the carried projection.
- Fail-closed: a foreign field is refused at all six levels (context, evidence,
  scope, projection, winner, provenance); scope kind/payload mismatches are
  refused; and semantically impossible envelopes are refused — abstaining but
  weighted, projecting but unweighted, projecting over `note_count: 0`, a
  note count contradicting its histogram, a sounding scope with no observed
  span, an inverted or out-of-range span, a tonic outside `0..=11`, a
  `scale_fit` outside `[0, 1]`, a negative margin, a margin that is not the
  winner-rival gap, a weighting branch the evidence never triggers, an unknown
  method, and a well-formed projection that is simply not what the carried
  evidence yields. One test guards the guard: a valid envelope still
  deserialises, whatever its key order.
- Root of trust: forged evidence is refused through both doors — duration mass
  on a silent scope, mass for a class that never sounded, mass beyond its
  onsets' ceiling, an onset with no representative in the span, and a span
  endpoint with no onset to stand on — including an octave span resting on a
  single onset, which its counterpart test pairs with the same span carrying
  two, so the rule refuses impossible spans rather than octaves. The sharpest
  case has its own test: forge the
  facts, then compute the estimate *honestly* from the forgery, so
  re-derivation is satisfied and only the evidence gate can catch it. Two more
  cover the arithmetic — an unsummable duration histogram is an `Err` from the
  wire, and `estimate_key` on the same histogram returns finite correlations
  rather than panicking.
- A proptest holds `measure` and `validate` together across all four scope
  shapes, so a gate this strict cannot quietly start rejecting real scores. It
  passes at the committed 512 cases and at a 20 000-case sweep
  (`PROPTEST_CASES=20000`).

### Known consequence

Validation by re-derivation means an artifact is readable only by a build whose
`TonalMethod` produces the same result. That is deliberate and fail-closed — a
document whose numbers this build cannot reproduce is refused rather than
half-trusted — but it does mean that changing the estimator must add a
`TonalMethod` variant with its own re-derivation path, not silently alter
`KsV1`. `TonalMethod` is the versioning hook for exactly this.

A second, far smaller limit is recorded rather than fixed.
`PitchEvidence::measure` tallies `onset_counts` as `u32` and `note_count` as
`usize`, each saturating independently, so past `u32::MAX` notes in a single
pitch class on a 64-bit target the two stop agreeing and `validate` would reject
a genuine measurement on the `note_count`-versus-histogram rule. Four billion
notes of one pitch class is unreachable for any real score, and the saturation
is Phase-1 behaviour, so it is left alone deliberately — but "`measure` is valid
by construction" holds below that bound, not above it, and the proptest does not
reach it either. The same note is in `PitchEvidence::validate`'s doc comment.

Implementation: `5d667ac` (red) and `af97684` (green) for the first cut;
`84bb992` (red) and `c9da896` (green) for the first review revision; `818f799`
(red) and `bc13ea2` (green) for the second, which validated the evidence itself; and
`76ba934` (red) and `4c1ccb7` (green) for the third, which made the
span-endpoint rule count multiplicity. Acceptance is a separate gate and has not
been given.

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
