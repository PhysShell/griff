# Proposal: Generator Reachability Lab

Deterministic coverage census and target-relative symbolic comparison.

Status: proposal for discussion (v2.1 — v2 revised per the PR #131 review;
supersedes the "Target Recovery Lab" discussion draft, whose research detail
survives in Appendix A)
Scope: standalone offline research instrument. No production generation
behaviour changes; `rerank.rs` and every frozen strategy stay untouched.

## 1. Goal

Measure the reachable output space of Griff's deterministic generators, and
compare generated monophonic fragments against eligible target fragments using
the existing explainable scoring conventions (ADR-0017).

Two questions, in priority order:

1. **Census.** What set of *unique* musical results does each strategy
   actually cover, and how fast does that set saturate as trials grow?
2. **Target-relative comparison.** Given an eligible target fragment, how
   close do the nearest candidates get — measured as named axes, never as one
   aggregate number?

Exact recovery is retained as a boolean control fact: a sanity check on
synthetic reachable targets, and a rare event to be reported honestly on real
holdout targets. It is not the headline product; the census is.

## 2. What changed from v1 (decision record)

- **Census first**, not exact recovery and not a surrogate model.
- **Reuse the existing scoring architecture** (`Axes`, versioned
  `WeightPolicy`, `Rationale`, `Provenance`, `Scored<T>`); add only genuinely
  missing event-level measurements. v1 was written green-field and ignored
  this layer — its main defect.
- **Directed search is evidence-gated**: it opens only after a benchmark shows
  exhaustive/random evaluation is a material bottleneck. The generation
  pipeline is cheap deterministic Rust; the expensive resource in this project
  is human listening, not CPU.
- **Configuration-level aggregation**: raw seed is a reproduction key, never a
  feature, so per-trial outcomes are irreducibly noisy from a feature view.
  Any future model predicts distribution summaries per configuration.
- **Family-level dominance** (5–6 families), all fine-grained measurements
  always stored. An 18-axis Pareto front degenerates — almost everything is
  non-dominated — and hypervolume there is both meaningless and exponential.
- **Alignment deferred** (Phase 1.5, evidence-gated). Most useful axes are
  timeline-based and need no alignment at all.
- **JSONL only**; Arrow/Parquet contradicts the lean dependency posture and is
  reconsidered only against measurements.
- **Roadmap placement decided by an audit first**; no stage number is assigned
  or invented now (glossary §0 rule).
- Renamed: exact match is not the promise. "Generator Reachability Lab".

Revised in v2.1 (PR #131 review):

- **Raw error diagnostics and ranking axes are separate contracts.** Only
  bounded, direction-aligned (higher-is-better) axes enter `Axes`, family
  views, and dominance; raw MAEs and counts are stored facts, never ranked.
- **Holdout is provenance-first.** Source-identity exclusion happens before
  corpus material is constructed; representation fingerprints per input
  channel are defence-in-depth, not the primary mechanism.
- **Measurement policy is its own versioned contract**, distinct from
  `WeightPolicy`, which keeps versioning only weights for views/ranking.
- **The Lab owns its JSONL wire DTOs**; core `Axes` gains no serde.
- **Exact match is symbolic-grid exactness**; tempo is excluded, meter is the
  ordered per-bar time-signature sequence.

## 3. Relationship to the existing metric layer

Phase 0 turns this section into a binding per-axis triage; the direction is
fixed here.

| Module | What it provides | Decision |
| --- | --- | --- |
| `scoring.rs` | `Axis`/`Axes`, versioned `WeightPolicy` (uniform-baseline convention), `Rationale`, `Provenance`, `Scored<T>`, aggregate-as-derived | **Reuse fully.** Comparison facts are plain `Axes`; measurement provenance lives in the run manifest (§4); no parallel `SimilarityVector` vocabulary. |
| `similarity.rs` | Chunk similarity v3 over persisted `ChunkMeta` metadata (structure, tags, gesture, complexity). Reads **no note content** by design. | **Reuse the contract, not the measurements.** Named axes + per-axis rationale + versioned policy, in a distinctly named domain: `target_comparison` v1 — not `similarity` v4. Chunk-level axes are never presented as note-level similarity. |
| `novelty.rs` | Top-melodic-line extraction, transposition-aware interval transitions on a common IOI grid (480/quarter), longest common contiguous run, n-gram novelty. Extraction helpers currently private. | **Partially reuse.** Directly relevant to interval contour, transposition-invariant comparison, and leakage/quote detection. Phase 0 decides per helper: reuse as-is / extract a shared internal primitive / extend / keep separate with explicit justification. No third slightly-different definition of "the melodic line". |
| `syncopation.rs` | Displaced-beat fact (off-beat eighth struck, beat itself not struck), reduced to a binary `Syncopated` tag at threshold 0.25. | **Extend, don't compete.** Extract the raw `DisplacementProfile` fact (displaced/eligible beats, positions); keep the existing tag derivation as a policy over it; the Lab compares profiles, not tags. This refactors existing tagging behaviour, so it starts with characterization tests (SPEC hard rule 5). |
| `feature.rs` | Note count, event durations, pitch/velocity ranges. Fixes the semantics that silence is derived from events + master timeline, never stored. | **Reuse.** `SilenceSegment` exists only as a comparison projection, not a new canonical entity. |
| `rerank.rs` | Production candidate set; six explainable axes under uniform `generation_rerank` v1. | **Do not touch.** The Lab measures. No lab-derived weights flow into production reranking without a separate acceptance contract — that is S9's job. |

## 4. Vocabulary and core decisions

A comparison result carries **facts only**, in the `scoring.rs` vocabulary:

```rust
pub struct TargetComparison {
    pub exact_match: bool,
    pub axes: Axes,        // bounded, direction-aligned — see below
}
```

- **Measurement policy and weight policy are different contracts.** The
  existing `WeightPolicy` versions only weights and keeps its single job:
  family views and ranking. Everything that determines how the facts are
  *produced* — normalization, fingerprint algorithm, onset tolerance, pairing
  rule and tie-break, per-axis normalization formulas — is versioned by a
  separate measurement contract, whose provenance lives in the run manifest
  (not on every `TargetComparison`):

```rust
pub struct TargetMeasurementPolicy {
    pub id: &'static str,
    pub version: u32,
    pub normalization_version: u32,
    pub fingerprint_version: u32,
    pub axis_formula_version: u32,
    pub onset_tolerance_ticks: u32,
    pub pairing_rule: PairingRule,
    pub tie_break: PairingTieBreak,
}
```

  Reusing `WeightPolicy` for this would create exactly the near-synonym this
  proposal argues against; `scoring.rs` separates facts from weighting on
  purpose, and the measurement process needs its own versioned identity.

- **Raw diagnostics and ranking axes are separate.** ADR-0017 axes are
  normalised per-axis into `[0, 1]` with higher-is-better semantics, and the
  uniform-policy convention assumes that direction. Raw errors (MAE in ticks
  or semitones, unpaired-event counts) violate both properties — unbounded,
  and larger-is-worse — so they never enter `Axes`, families, or dominance.
  They are stored as facts:

```rust
struct RawDiagnostics {
    onset_mae_ticks: Option<f64>,      // None when no paired events —
    pitch_mae_semitones: Option<f64>,  // never NaN, never a fake zero
    unpaired_target_events: u32,
    unpaired_candidate_events: u32,
}
```

  Every ranking axis derived from a raw error (e.g. `onset_proximity` from
  `onset_mae_ticks`) has a documented range, a direction (higher is better),
  and a normalization formula versioned by `axis_formula_version`.

- **Exact match is symbolic-grid exactness.** Exact means identical symbolic
  placement on the normalized master grid, *not* identical wall-clock
  playback timing: the tempo map is deliberately excluded, and the same note
  grid at 80 and 160 BPM is the same fragment for recovery purposes. The
  signature is the ordered set of `(relative onset, duration, pitch)` after
  normalization, plus matching bar count, the **ordered per-bar
  time-signature sequence** (not one meter — targets may cross meter
  changes), and fragment bounds. Velocity is excluded in Phase 1: the
  current generator assigns velocity largely by strategy, so matching it
  would test the constants 80–92, not musical structure. It may return later
  as a separate axis.
- **Normalization vs comparison are separate enums.** Transposition
  invariance is a comparison mode / axis, never a normalization of the
  canonical target; a transposed candidate is never an exact match.

```rust
pub enum TimelineNormalization { Exact, CommonPpqn }
pub enum PitchComparisonMode { Absolute, TranspositionInvariant }
```

- **Fingerprint discipline.** A stable hash of the canonical signature is a
  filter; a hash hit triggers full structural verification, and only
  structural equality sets `exact_match = true`.
- **Raw seed is provenance**, a reproduction key and PRNG-trajectory id —
  never an ordinal feature.
- **Two-level dataset.** Per-trial facts and per-configuration summaries are
  both first-class, because any model that excludes the seed can only learn
  the configuration-level distribution:

```rust
struct TrialRecord {
    configuration_id: ConfigurationId,
    seed: u64,
    candidate_fingerprint: Fingerprint,
    axes: Axes,            // bounded ranking axes only
    raw: RawDiagnostics,   // unbounded error facts, never ranked
}

struct ConfigurationSummary {
    configuration_id: ConfigurationId,
    trial_count: usize,
    mean_axes: Axes,
    median_axes: Axes,
    p90_axes: Axes,
    per_axis_max: Axes,   // marginal maxima — may correspond to no single trial
    exact_hit_rate: f64,
    unique_candidate_rate: f64,
}
```

  `per_axis_max` is a marginal summary: each axis's maximum over the
  configuration's trials, which need not be realised by any one candidate.
  Referencing actual best candidates (e.g. `family_best_trial_ids` under a
  named `WeightPolicy`) is a Phase 0 design detail.

- **Reporting language.** "No exact match found within N trials" — never
  "unreachable". An unreachability claim requires full enumeration of a
  finite space or a structural proof about the generator's constraints.

## 5. Phase 0 — metric and expressivity audit

No production code. Deliverable:
`docs/audit/YYYY-MM-generator-reachability-metric-inventory.md`, answering:

1. **Inventory** of `scoring.rs`, `similarity.rs`, `novelty.rs`,
   `syncopation.rs`, `feature.rs`, `rerank.rs`, and the relevant
   structure/gesture metrics.
2. **Per-axis triage table** — every proposed fact gets an existing-source
   column and a decision: reuse / extend / new. Starter:

   | Proposed fact | Existing source | Decision |
   | --- | --- | --- |
   | Exact onset set | none | new |
   | Exact pitch on paired onsets | `novelty` line extraction partly relevant | extend |
   | Interval contour | `novelty` transitions | reuse/extend |
   | IOI sequence | `novelty` normalized IOI grid | reuse/extend |
   | Silence occupancy | master timeline + events (`feature.rs` semantics) | new projection |
   | Syncopation | `syncopation.rs` | extend (raw `DisplacementProfile`) |
   | Pitch range | `feature.rs` | reuse |
   | Structure/repeatability | existing structure metrics | reuse where semantically applicable |

3. **Holdout feasibility check.** Verify that song/chunk identities and
   source ranges survive the current corpus-loading path end-to-end, so that
   provenance-first exclusion (§6, holdout discipline) is actually
   enforceable before material construction. If they are lost anywhere, fixing
   that is a Phase 1 prerequisite.
4. **Target eligibility contract.** Phase 1 targets are monophonic
   `ExactVoice` projections only; every target carries an explicit
   eligibility/projection record; polyphonic/chordal/technique-bearing
   fragments are ineligible until the generator's output space contains such
   objects at all.
5. **Cost benchmark.** Measured cost of generation only, generation +
   fingerprint, and generation + full metrics at 1k / 10k / 100k trials.
   This number is the gate for any future directed-search discussion.
6. **Roadmap placement recommendation.** One of: a small
   regression/evaluation slice tied to S6/S9; an ADR for a new
   infrastructure boundary; or a new stage appended per the canonical
   glossary §0 rule. No number is assigned before that decision.

## 6. Phase 1 — minimal reachability census

### In scope

- one eligible monophonic target; common-PPQN projection;
- canonical signature (onset, duration, pitch) + stable fingerprint;
- exact structural verification after a hash hit;
- fingerprint for every generated candidate;
- unique-candidate count, duplicate rate, trials-to-uniques curve, per-strategy
  breakdown;
- holdout modes (below); reproducible provenance; JSONL storage;
- MIDI export of top results;
- a small set of alignment-free axes.

### Fine-grained measurements (always stored)

Bounded ranking axes, all in `[0, 1]`, higher is better, each normalization
formula versioned by the measurement policy:

```
onset_similarity            // F1 over tolerance-window onset matches
onset_proximity             // normalised from onset MAE
pitch_exact_on_paired_onsets
pitch_proximity             // normalised from pitch MAE
duration_similarity, ioi_similarity,
occupancy_iou, silence_iou, rest_boundary_f1,
displacement_profile_similarity,
note_count_similarity
```

Raw diagnostics, stored alongside but never ranked, weighted, or entered
into dominance: `onset_mae_ticks`, `pitch_mae_semitones`, unpaired-event
counts, per-onset true/false positive counts.

**Onset pairing rule** (instead of alignment): exact onset match pairs first;
otherwise the nearest onset within a tolerance window; each candidate event is
used at most once; remaining events count as insertions/deletions; temporal
order is never rearranged. Ties are broken deterministically, and the
tie-break rule is part of the versioned measurement policy — a metric that
depends on iteration order is not reproducible.

### Summary families

For dominance and top-K only: `rhythm`, `pitch`, `duration`, `silence`,
`syncopation`. `structure` is added only if Phase 0 shows the existing
structure metrics apply to short fragments without semantic distortion.

Family projections are `WeightPolicy` instances over the **bounded ranking
axes only** — never over raw diagnostics — and start **uniform**, per the
house convention (`generation_rerank` v1 is uniform; non-uniform weights need
evidence, not taste). Uniform weighting is a well-defined neutral baseline
here — every input axis is `[0, 1]` and direction-aligned — which makes it
mathematically coherent, not a claim that the axes are independent or equally
salient to the ear. A family score is a view; the stored measurements remain
the truth.

Pareto handling: no hypervolume. Deliverables are the top result per family,
a small epsilon-Pareto set over the families, the count of non-dominated
candidates, and a deterministically capped archive.

### Storage

```
runs/
  manifest.json    // commit SHA, strategy versions, full
                   // TargetMeasurementPolicy (id + all versions), family
                   // WeightPolicy versions, target fingerprint, corpus fingerprint,
                   // parameter-space definition, sampler config, experiment
                   // seed, trial count, timestamps
  trials.jsonl
  summary.json
  artifacts/*.mid
```

**The Lab owns its wire format.** Core `Axis`/`Axes` are an internal scoring
vocabulary — `&'static str` labels, no `Serialize` — and stay that way: no
serde enters `core::scoring` for the Lab's sake. The Lab/CLI defines its own
DTOs that mirror `Axes`, the same way the cockpit owns its sidecar contracts:

```rust
#[derive(Serialize)]
struct StoredAxis {
    label: String,
    value: f64,
}
```

### Holdout discipline

```rust
pub enum CorpusMode { NoCorpus, HoldoutTargetSong, HoldoutTargetFragment, LeakyDiagnostic }
```

The production pipeline feeds corpus chunks in as rhythm templates and novelty
references, so target leakage is the default behaviour, not a hypothetical.

**Holdout is enforced by source identity before material construction;
representation fingerprints are defence-in-depth, not the primary exclusion
mechanism.** A single full-fragment fingerprint cannot carry holdout on its
own, because the target reaches the generator in several lossy
representations: a rhythm template carries onsets and durations but no
pitches, so an `(onset, duration, pitch)` fingerprint never matches it; and
novelty references compare `(pitch interval, normalized IOI)` transitions,
which are deliberately transposition- and PPQN-robust, so a transposed leak
slips past an absolute fingerprint by design.

The leakage contract is therefore channel-specific:

- **Source provenance (primary):** every corpus contribution carries song id,
  chunk id, and source range; `HoldoutTargetSong` / `HoldoutTargetFragment`
  exclude material by these identities *before* rhythm templates or novelty
  references are compiled.
- **Rhythm channel (defence-in-depth):** rhythm-grid fingerprint of the
  target (onsets + durations, no pitch) checked against every supplied
  template, plus an onset/duration subsequence check.
- **Melodic-reference channel (defence-in-depth):** absolute fragment
  fingerprint, an interval + normalized-IOI fingerprint, and an
  n-gram/contiguous-run check against novelty references (reusing the
  `novelty.rs` transition machinery).

`LeakyDiagnostic` runs are explicitly labelled control experiments. Phase 0
must verify that source identities actually survive the current
corpus-loading path; if they do not, that is a Phase 1 blocker to fix first.

### Out of scope for Phase 1

Sequence alignment and split/merge; Parquet; hypervolume; Random Forest /
Extra Trees; directed sampling; preference learning; production rerank
changes; a new stage without the roadmap decision from Phase 0.

## 7. Phase 1.5 — alignment, only if evidence requires it

Opens only against a fixture set of real misalignment cases — e.g. best
candidates systematically representing one long note as two (or the reverse)
so that onset pairing distorts the comparison. Then a Mongeau–Sankoff-style
alignment with hard temporal-order and onset-distance constraints; split/merge
operations after that, under the same evidence rule. Timeline occupancy and
silence axes stay alignment-independent regardless.

## 8. Phase 2 — multi-target regression

- a set of holdout fragments; held-out songs;
- strategy-version comparison and coverage regression over time;
- metric stability checks across runs;
- family summaries per strategy;
- bounded artifact retention.

## 9. Future ML

Tabular models may later be evaluated for configuration-level sensitivity
analysis, prediction of distribution summaries, or preference learning
(S9 territory — the scarce resource is human listening). Directed sampling
requires prior evidence that brute-force evaluation is a material bottleneck
(Phase 0 cost benchmark, re-measured if generation grows DP/fretboard-search
stages). Raw seed is provenance, not an ordinal feature. Nothing in Phases
0–2 depends on any of this existing.

## 10. Acceptance criteria (Phase 1)

1. The same manifest reproduces the same fingerprints and axes, or reports a
   typed reproducibility failure.
2. The exact matcher distinguishes full equality from a change to a single
   onset, duration, or pitch.
3. A transposed, rhythmically identical candidate is not an exact match but
   scores high on `ioi_similarity` and the rhythm-family axes and low on
   `pitch_exact_on_paired_onsets` / `pitch_proximity`.
4. Moving a rest changes `silence_iou` and/or `rest_boundary_f1`.
5. A displaced (anticipated) variant and its on-beat counterpart differ in
   `displacement_profile_similarity`.
6. The census counts unique canonical fingerprints, not files.
7. A trials-to-uniques curve is produced per strategy and overall.
8. A synthetic reachable target produced by a known trial is recovered
   exactly when its exact producing trial — configuration *and* seed — lies
   in the sampled trials. (Configuration alone guarantees nothing: the same
   configuration under a different seed is a different candidate.)
9. Holdout excludes target material by source identity before rhythm
   templates and novelty references are compiled; the channel-specific
   representation checks (rhythm-grid, interval + normalized-IOI) run as
   defence-in-depth; any run that bypasses either layer is labelled
   `LeakyDiagnostic`.
10. No production generation path changes; `rerank.rs` is untouched.
11. No report claims unreachability.
12. Top-K results export to MIDI with full provenance.
13. The measurement policy (normalization, fingerprint, tolerance, pairing,
    axis formulas) and the family weight policies are separately versioned,
    and the run manifest records both.
14. Every benchmark target has an eligibility/projection record.

## 11. Risks

- **Metric gaming** — store the full vector, listen to top-K exports, later
  compare against S9 preference data.
- **Target leakage** — holdout modes, fingerprint checks, explicit labelling
  of leaky control runs.
- **False diversity** — canonical fingerprints, unique-candidate census.
- **Parallel metric vocabulary** — Phase 0 triage; terms extend the glossary
  rather than acquiring synonyms.
- **Premature ML** — Phases 0–2 are ML-free; directed search is gated on the
  measured cost benchmark.

## 12. Non-goals

Neural embeddings or end-to-end neural generation; automatic changes to
production weights or frozen strategies; polyphonic, chord, guitar-technique,
fretboard-fingering, articulation, or audio similarity; a single universal
similarity aggregate; cockpit integration as the first target; storing
millions of full `Score`s; imitation of a named artist as a legal or
marketing claim; proof of musical quality.

## Appendix A — research notes (retained from v1, future work)

Prior art surveyed for the v1 discussion draft, kept as references for the
evidence-gated later phases (prior-art-first rule, `AGENTS.md`):

- Mongeau, M., & Sankoff, D. (1990). "Comparison of Musical Sequences."
  *Computers and the Humanities*, 24(3), 161–175. doi:10.1007/BF00117340.
  Borrowed: the pitch- and rhythm-aware edit-distance formulation, as the
  starting point *if* Phase 1.5 alignment opens on evidence. Not borrowed:
  their consolidation/fragmentation operations in Phase 1 — split/merge is
  explicitly deferred.
- Typke, R., Giannopoulos, P., Veltkamp, R. C., Wiering, F., & van
  Oostrum, R. (2003). "Using Transportation Distances for Measuring Melodic
  Similarity." *Proceedings of ISMIR 2003*, Baltimore, USA. Borrowed: the
  framing of notes as weighted points in onset/pitch space for joint
  time/pitch/duration comparison. Not borrowed: transportation distance as a
  Phase 1 metric — the census axes stay simpler and cheaper.
- Janssen, B., van Kranenburg, P., & Volk, A. (2017). "Finding Occurrences
  of Melodic Segments in Folk Songs Employing Symbolic Similarity Measures."
  *Journal of New Music Research*, 46(2), 118–134.
  doi:10.1080/09298215.2017.1316292. Borrowed: the empirical argument that
  representation and method choice materially change similarity outcomes —
  the standing case against a single universal metric and for the axis
  vector. Not borrowed: their specific alignment/wavelet/structure-induction
  pipelines.
- Bemman, B., & Christensen, J. (2024). "Inner Metric Analysis as a Measure
  of Rhythmic Syncopation." *Proceedings of the 25th International Society
  for Music Information Retrieval Conference (ISMIR 2024)*, San Francisco,
  USA, 389–396. Borrowed: as a future validation reference for
  `displacement_profile_similarity` — they report Spearman r_s = 0.80
  against perceptual syncopation ratings on the Song (2014) set of 111
  single-bar patterns. Not borrowed: Inner Metric Analysis itself in
  Phase 1; the displacement profile remains an engineering hypothesis, not
  established perceptual truth, until validated against work of this kind.
