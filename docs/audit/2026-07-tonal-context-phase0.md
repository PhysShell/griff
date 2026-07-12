# Tonal context — evidence / inference layer, Phase 0 design (2026-07)

Status: **design note (not an ADR yet)** — no behavior change lands with it.
Input: the arbiter's follow-up to the accepted register track. The register
work made `PitchMaterial.root` an explicit pitch-class *anchor*, **not** a
tonic; cadence-aware endings are frozen precisely because no real tonal center
exists to cadence onto. This note audits the current contract, proposes a
typed evidence/inference layer with explicit uncertainty, and fixes a synthetic
test plan — **without** writing production tonal inference. Production
implementation waits on local `tonal_evidence` scan numbers.

Heuristics-first (ADR-0008 / S12 gate): the inference is a
Krumhansl–Schmuckler correlation, never ML. This note reuses the estimator
griff already has rather than inventing a second one (the "one mapper"
principle the register track settled).

## 0. One-line

Split *evidence* (pure, observed pitch-class facts) from *inference* (a scored,
uncertain key estimate) as shared pure-core types; promote the existing
private, single-winner `complement::estimate_harmony` into that shape with a
candidate list and an explicit confidence margin — later, gated on corpus data.

## 1. Current contract (audit — what is true today)

### 1.1 How generation seeds pitch material

`griff_cli::generation_input::generation_request_from_score`:

- gathers **all** note pitches across **all** tracks and voices
  (`all_pitches`);
- pitch range = the global `(min, max)` of those pitches
  (`pitch_range` → `constraints.pitch_lo/hi`);
- `PitchMaterial.root` = the **global minimum** pitch;
- `PitchMaterial.intervals` = the distinct pitch classes, expressed as
  semitone offsets from that minimum (`pitch_material_from`);
- `root` is an **anchor** that only contributes its pitch class to
  `PitchMaterial::pitch_classes()` — it is **not** a tonic (register track,
  accepted 2026-07-12).

So the generator today has a pitch-class *palette* and a *range*, and no notion
of a tonal center, key, or mode. Every strategy walks the `ScaleLadder` built
from that palette; nothing knows which class is "home".

### 1.2 Tonal inference already in-tree (do not re-derive)

`core/src/complement.rs` already estimates a key:

- `estimate_harmony(notes) -> Option<HarmonicContext>` (`pub(crate)`), using
  the **Krumhansl–Schmuckler** algorithm: a **duration-weighted** pitch-class
  histogram correlated (Pearson) against the 24 rotated Krumhansl–Kessler
  major/natural-minor profiles; the single best correlation wins, ties broken
  by earliest key in a major-then-minor, C-upward scan;
- returns `HarmonicContext { tonic_pitch_class, mode: KeyMode, scale_fit }`,
  carried on `PartProfile.harmony` and consumed by the ComplementArranger;
- glossary §8 already names this "Harmonic context" and marks `scale_fit` a
  *fact*, not a verdict (fit thresholds are corpus/S9 calibration).

**The gap this note addresses** — the existing estimator:

1. is private to `complement`, so generation cannot reuse it;
2. returns a **single winner**, with no runner-up and no confidence margin —
   it cannot express *ambiguous* or *modulating*;
3. weights by **duration only** — onset salience is not separable;
4. is **part-scoped** — there is no whole-score / per-track / per-voice
   distinction;
5. mixes measurement and inference in one call (no reusable *evidence*).

Phase 1 is to close (1)–(5) by generalising this one estimator, not adding a
second.

## 2. Design — evidence vs inference (typed, pure-core)

Two layers, deliberately separated so measurement is a pure fact and inference
is a scored, *uncertain* verdict (mirroring ADR-0017's axes-vs-aggregate split
and the `StructureMetrics`-vs-`StructureControl` duality).

### 2.1 Evidence (facts, pure, deterministic)

```
struct PitchEvidence {
    scope: EvidenceScope,
    note_count: usize,
    sounding_ticks: u64,               // total sounded duration in scope
    pitch_range: Option<PitchRange>,   // None when the scope is silent
    onset_pc_weights: [f64; 12],       // per-class count of note onsets
    duration_pc_weights: [f64; 12],    // per-class sounded duration
}

enum EvidenceScope {
    WholeScore,
    Track(usize),
    Voice { track: usize, voice: usize },
}
```

`PitchEvidence` is a pure projection of a `Score` region — no thresholds, no
key. The two histograms are kept separate because onset salience and sustained
duration disagree (a pedal tone dominates `duration_pc_weights` but not
`onset_pc_weights`); the inference weights them, evidence does not. The
existing estimator's duration histogram is exactly `duration_pc_weights`; the
onset histogram is the new axis.

### 2.2 Inference (scored, uncertain)

```
struct TonalCandidate {
    tonic: PitchClass,   // 0..=11
    mode: Mode,          // major / natural-minor (extendable)
    score: f64,          // correlation against the rotated profile
}

struct TonalEstimate {
    candidates: Vec<TonalCandidate>,   // best-first, at least the top few
    confidence_margin: f64,            // best.score - runner_up.score
    evidence_scope: EvidenceScope,
}
```

`TonalEstimate` carries **explicit uncertainty**: a `confidence_margin` (the
gap between the winner and the best rival key) plus the full ranked list, so a
caller can distinguish *high confidence* (large margin) from *ambiguous* (near
tie) without re-running the maths. `HarmonicContext` becomes a lossy projection
of a `TonalEstimate` (its winner's tonic/mode + `scale_fit`), so complement
keeps its current output while generation gets the richer shape.

Names are not binding; the two invariants are: **evidence separated from
inference**, and **uncertainty explicit** (never a bare single key).

### 2.3 Where scoring weights live

The correlation weights (Krumhansl–Kessler profiles) and the onset-vs-duration
blend are **data**, not code (ADR-0017 §3): a named, versioned policy the S9
feedback layer can tune, the same posture as every other griff scorer. Phase 0
does not fix the blend — the synthetic scan (below) informs it.

## 3. Scope guardrails — nothing changes yet

This increment adds **only** this note and the test plan. Explicitly **not**
touched:

- `RuleGenerationRequest` — no `TonalCenter` field;
- `PitchMaterial`, `ScaleLadder`, and the five generation strategies;
- the reranker and its weights (no register-coherence axis, no tonal axis);
- cadence — stays frozen until a real `TonalEstimate` is available to cadence
  onto, and even then behind its own increment.

`estimate_harmony` / `HarmonicContext` stay exactly as they are until Phase 1.

## 4. Synthetic test plan (Phase 0 deliverable)

The estimator must earn trust on constructed inputs before any corpus number.
Each case fixes the *expected uncertainty class*, not a hard threshold (the
thresholds are what the scan calibrates):

| # | Synthetic input | Expected verdict |
|---|-----------------|------------------|
| 1 | Clean C major (diatonic, tonic-weighted) | **high confidence** — winner C major, wide margin |
| 2 | Clean A minor (natural, tonic-weighted) | **high confidence** — winner A minor; C-major relative is the runner-up, margin non-trivial |
| 3 | Pentatonic material (C D E G A) | **low confidence** — C-major favoured but small margin (pentatonic underdetermines major/minor) |
| 4 | Chromatic material (all 12 classes ~even) | **ambiguous** — flat histogram, near-tie candidates, margin ≈ 0 |
| 5 | Two tracks in conflicting keys (C major + F# major) | scope-dependent: `WholeScore` → **ambiguous / low**; each `Track` → **high** for its own key |
| 6 | Melodic guitar (clear key) + chromatic percussion/noise track | `WholeScore` degraded by noise → **low**; melodic `Track` → **high** — motivates scoping the evidence |
| 7 | Short tonic pedal (few onsets, one long sustained tonic) | **low confidence** and/or *unsupported* — `note_count`/`sounding_ticks` too thin; onset vs duration disagree |
| 8 | Modulating two-section score (C major → G major) | `WholeScore` → **ambiguous / modulating**; per-section (future windowed scope) → two **high**-confidence estimates |

Cases 5–8 are the reason `EvidenceScope` and `confidence_margin` exist:
whole-score inference must be *allowed to be uncertain*, and per-scope evidence
must be reachable. An `unsupported` outcome (too few notes / ticks to estimate)
is distinct from `ambiguous` (enough data, no clear winner) — both are honest,
neither is a silent guess.

## 5. External inspiration — borrow the decomposition only

From **AugmentedNet** (Roman-numeral analysis network) take *only* the output
**decomposition**: `key / root / degree / quality / confidence` as the
vocabulary a tonal estimate should expose (it confirms that *confidence* and a
separable *key/root* are the right surface). Reject the rest for our specifics:

- **no TensorFlow / neural runtime** — violates ADR-0008 / the S12 heuristics
  gate; the corpus for training does not exist;
- **no MusicXML inference runtime** — griff's boundary is MIDI/GP → canonical
  model, not MusicXML;
- **no Roman-numeral / functional-harmony model** — degree/quality beyond
  tonic+mode is far past what generation needs now; `TonalCandidate` stays
  tonic + mode until a concrete need appears.

The actual estimator stays the Krumhansl–Schmuckler correlation already in
`complement`.

## 6. What lands where (gated on the local scan)

- **Phase 0 (this note):** contract audit, typed evidence/inference design,
  synthetic test plan. No code.
- **Phase 1 (landed 2026-07-12, `core/src/tonal.rs`):** a pure-core
  evidence/inference module — `PitchEvidence` measurement + a `TonalEstimate`
  inference promoted from `estimate_harmony`, with the candidate list and
  confidence margin; `HarmonicContext` re-expressed as its projection
  (characterization tests, no golden change to complement). No
  generation/reranker/cadence change. See §7 for what shipped vs. this sketch.
- **Later (own increments):** a scoped `TonalEstimate` on the generation input;
  only then does cadence unfreeze, and only behind a confidence gate (a
  low-confidence or ambiguous estimate must *not* force a cadence onto a
  guessed tonic — the register track's "no silent fallback" rule applies to
  tonality too).

## 7. Phase 1 amendments (landed 2026-07-12)

Phase 1 shipped as the pure-core module `core/src/tonal.rs` (red suite
`core/tests/tonal.rs`); `complement::estimate_harmony` now projects the winning
candidate of a `tonal::TonalEstimate` and its characterization tests are
unchanged. The shipped shape refines the §2 sketch in a few honest ways,
recorded here rather than rewritten in place so the design history stays
readable.

**What shipped (vs. the §2.1–§2.2 sketch).**

- `PitchEvidence` carries *raw integer* histograms, not the `f64` `*_pc_weights`
  of the sketch: `onset_counts: [u32; 12]` and `duration_mass: [u64; 12]`, plus
  `note_count` and the observed `feature::PitchRange`. Evidence stays raw; the
  inference resolves the weighting. There is no separate `sounding_ticks` field
  — per-class `duration_mass` carries it and its sum is the scope total.
- `TonalCandidate` is `{ tonic, mode: KeyMode, correlation, scale_fit }`. It
  reuses the existing `KeyMode` (not a fresh `Mode`), and — beyond the sketch —
  *every* candidate carries its own `scale_fit`, not only the winner.
- `TonalEstimate` is `{ candidates (24, best-first), confidence_margin }`. The
  scope is **not** duplicated onto the estimate; it lives on the `PitchEvidence`
  that produced it. This also lets `estimate_harmony` — which holds weighted
  notes and no scope — share the one inference core without inventing a scope.
- KS v1 landed exactly as contracted: duration mass weights the histogram, raw
  onset counts are the fallback only when the total duration mass is zero, and
  there is no onset/duration blend and no metric-accent policy.

**What these facts are — and are not.**

1. A key result on real-song input is a *tonal hypothesis for a scope*, not
   verified truth. The estimator earns trust on the synthetic cases (§4); a key
   it reports over a corpus track is the winning correlation, not a
   ground-truth label.
2. `onset_counts` are **raw facts** — literal per-class onset tallies, carrying
   no weighting or normalisation.
3. `duration_mass` is **duration mass** — summed sounded ticks — **not**
   wall-clock sounding time; ties, overlaps and tempo are not modelled here.
4. Every `TonalCandidate` contains `scale_fit`, the weighted on-scale fraction
   for that key — a per-candidate fact, not a verdict.
5. Confidence thresholds and automatic scope selection remain **uncalibrated**.
   Phase 1 exposes the margin and the per-scope evidence but sets no
   High/Low/Ambiguous cutoff and makes no automatic whole-score-vs-track
   choice. The local margins observed on the synthetic fixtures (diatonic
   ≈ 0.076, pentatonic ≈ 0.085) are diagnostics, not thresholds.

## 8. Sources

- C. Krumhansl & E. Kessler, probe-tone key profiles, *Psychological Review*
  89, 1982 (the profiles already used by `estimate_harmony`).
- D. Temperley, *The Cognition of Basic Musical Structures*, MIT Press, 2001
  (Krumhansl–Schmuckler key-finding and its known pentatonic/modal failure
  modes — cases 3 and 8 above).
- N. Nápoles López et al., *AugmentedNet* (2021) — borrowed for its
  key/root/degree/quality/**confidence** output decomposition only.
- ADR-0008 (heuristics before ML), ADR-0017 (axes vs aggregate; weights as
  data), glossary §8 (Harmonic context).
