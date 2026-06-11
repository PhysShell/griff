# Melodic closure & completeness research — borrow / adapt / reject analysis (2026-06)

Status: research note (not an ADR). Input: a design concern raised in
discussion — *"the generator might emit fragments (three notes and awkward
rests) that feel broken off, while a real DGD-style burst-and-rest phrase
feels finished; I can't explain the feeling, and I'm afraid of ending up with
a nonsense generator"* — investigated against the music-cognition literature
on melodic closure / phrase completeness and the music-generation literature
on structure and originality evaluation, then evaluated against what griff
already has (SPEC, S4/S6/S14, ADR-0008/0017).

The job of this note: name the phenomenon, list its measurable components
with sources, and pin each actionable idea to the stage/ADR where it lands —
**without** letting the generator reproduce real-song fragments.

## 0. The one-line conclusion

"Feels finished" is not mystical — it decomposes into five measurable cue
families (realized expectation, tonal stability of the ending, phrase-final
lengthening, metrical placement of rests, gesture recurrence), four of which
griff already computes or has canonized; the genuinely new items are a
**closure axis** under the ADR-0017 scoring vocabulary — computable today by
re-using the S4 boundary detector as the generation-side referee — and a
**novelty guard** (interval+rhythm n-gram / longest-common-subsequence
overlap cap against the corpus) that gives the glossary's `novelty` axis a
concrete measure.

## 1. The phenomenon: melodic closure

Music cognition has studied "this melody is finished" for ~70 years under the
name **closure** (at phrase level: *completeness*). The robust, repeatedly
replicated cue families:

1. **Realized expectation (implication-realization).** A melody implies its
   continuation; closure arises when the implication resolves: direction
   change after a leap, a large implicative interval followed by a smaller
   realized one, a filled gap. An unfilled leap or unresolved implication is
   exactly the "broken-off stub" feeling (Meyer 1956; Narmour 1990/1992;
   Schellenberg's two-factor simplification 1996/1997).
2. **Tonal stability of the ending.** Endings on stable scale degrees (tonic,
   then third/fifth) sound closed; unstable degrees hang. The
   Krumhansl–Kessler probe-tone profiles are a 12-number table per key —
   trivially implementable and explainable.
3. **Phrase-final lengthening.** The last note of a finished phrase tends to
   be lengthened and/or followed by a rest at a metrically meaningful
   position (GTTM grouping preference rules; Tenney & Polansky 1980; LBDM).
   Corollary for burst-and-rest writing: **a rest reads as part of the
   gesture (phrasing) when it is metrically predictable, and as absence (a
   hole) when it is not.**
4. **Contour: the melodic arch.** Phrases are statistically arch-shaped, and
   descending endings close harder (Huron 1996; *Sweet Anticipation* 2006).
5. **Recurrence makes intention.** Repetition is what turns sound into
   perceived musical intention (Margulis 2014; Deutsch's speech-to-song
   illusion). A one-off 5-note burst with a rest is a fragment; the same
   gesture recurring 2–4 times, varied, is a style. **Completeness lives
   substantially at the structure level, not only inside the phrase** — the
   single most important point for the DGD burst-and-rest case.

The statistical-expectation line quantifies family 1: IDyOM (Pearce) computes
per-note information content from corpus n-gram statistics; IC spikes predict
*perceived* phrase boundaries with F1 ≈ 0.58–0.64 across studies — note that
the S4 acceptance bar (F1 ≥ 0.7 on the hand-labelled corpus) sits *above*
published perception-model results. Gestalt-style heuristics and statistical
learning predict expectations comparably well (Cognition 2019), which backs
the ADR-0008 heuristics-first posture with literature rather than taste.

The generation-side mirror of the concern is the "noodling problem" — locally
plausible notes without global intention — the central theme of the
structure-modelling survey literature (arXiv 2403.07995, 2509.00051).

## 2. Where griff already stands (do not re-derive)

| Closure cue | Already in griff |
| --- | --- |
| Segmentation cues: pause / cadence / rhythm-reset / register-jump / density-change | S4 `BoundaryReason` — the six weighted signals in `core/src/boundary.rs` |
| Cadence-aware endings | S6 strategy list; glossary §2 (Cadence, Rhythmic reset) |
| Gesture recurrence / period / repeatability / loop seam | S14 `StructureMetrics` (Phases 0–2 landed) |
| Axes + weights-as-data + rationale (no magic scalars) | ADR-0017 |
| Weights as corpus-calibrated placeholders, validated by blind listening | S4 open question; S6 acceptance (≥ 60 % blind listen); S9 tuning |
| Schema-level corpus reuse (rhythm skeletons, new pitches) | S6 `RhythmCopyPitchSubstitute` |

## 3. Borrow — high value, fits our specifics

### 3.1 Closure as a scored axis via detector–referee symmetry

The S4 boundary detector and a closure scorer are the same measurement read
from opposite sides: generate a candidate, run the detector over it, and the
closure axis is the boundary score at the *intended* phrase end minus a
penalty for spurious high-score boundaries inside the phrase. This re-uses
the trick S14 already ratified — the tile/vary compiler is graded by the same
contour-aware similarity it optimises, so compiler and referee agree by
construction (decisions.log 2026-06-09 / -10). Lands at: the ADR-0017 scoring
vocabulary; reranking of S6 / S14 candidate sets; later a neighbour of
`phrase_continuity` in the S7 DP cost.

### 3.2 Ending-stability heuristic (Krumhansl profile)

Score the landing note of a phrase by scale-degree stability, weighted by
metrical position and final lengthening. Cheap, explainable ("ended on the
♭9 — unstable"), and lands directly in the S6 "cadence-aware endings" slot as
a named component of the closure axis.

### 3.3 Gap-fill / reversal mini-rules (Narmour)

Two rationale-friendly booleans: *leap resolved by direction change* and
*registral return after an extreme*. Components of the closure axis, per the
ADR-0017 rationale contract.

### 3.4 Novelty guard — n-gram / LCS overlap against the corpus

The generation-evaluation literature's standard originality measures: the
longest subsequence found verbatim in the training data, and the share of
n-grams shared with the source corpus. Adopt as: interval+rhythm n-gram
overlap against the S5 corpus manifest; the `novelty` axis (glossary §8
quality vector) gets this as its measure; candidates exceeding a
verbatim-match threshold (order of 1–2 bars) are flagged / rejected at the
caller. This resolves the explicit product requirement: *learn schemata from
real songs, never emit their fragments.* Deterministic and explainable.

### 3.5 Burst-and-rest gesture statistics (the DGD case)

What the corpus should teach for burst writing is distributions, not content:
burst length (the 5–8-note flurry), rest placement relative to the metrical
grid, landing-degree distribution, final-lengthening ratio. These join the
numeric chunk axes persisted in S14 Phase 3 and become S6 constraint inputs.

## 4. Adapt with caution

- **IDyOM-lite (n-gram expectancy).** An n-gram expectancy model is *not*
  neural and is cheap on a micro-corpus, but it is a trained artifact with
  provenance / versioning obligations. Park behind S9 data; revisit before
  S12 as the quantitative big brother of the closure axis.
- **Phrase-final lengthening as a hard rule.** In a heavily syncopated idiom
  the "strong beat" is often displaced (anticipation / push — the
  rhythmic-device axis of
  [`2026-06-expressive-control-and-scoring.md`](2026-06-expressive-control-and-scoring.md)
  §2.5). Encode as a soft axis; corpus calibration decides its weight (the S4
  weights are placeholders by design).

## 5. Reject for our specifics

- **Audio-domain closure cues** (decay, breath, reverb tails) — the symbolic
  boundary holds (same reasoning as the AudioMuse-AI survey decision,
  decisions.log 2026-06-10).
- **A trained closure classifier now** — violates heuristics-before-ML
  (ADR-0008; S12 hard gate). The closure axis must be rule-based first.
- **Verbatim corpus patterns as a musicality guarantee** — explicit product
  requirement to the contrary; the novelty guard exists precisely so corpus
  learning stays at the schema level.

## 6. Honest limits

Guitar-specific closure-perception research is essentially absent; the cue
families above were validated on folk / vocal / classical melodies. The
families are style-general; the *weights* are not. Whether a syncopated burst
landing on a ninth counts as closed **in swancore** is exactly what the S5
corpus, the S6 blind-listen gate, and S9 feedback are for: the literature
supplies the axes, the corpus supplies the weights — already the canon's
stance (S4: "calibrate on S5 corpus").

## 7. Actionable backlog (mapped, ordered)

1. **Glossary**: add *Closure / Completeness* to §7 with the five cue
   families (doc-only increment, glossary DoD §20). ✅ landed 2026-06-10.
2. **Closure axis v1**: S4-referee score + ending-stability + gap-fill
   booleans, as a named, versioned `WeightPolicy` under the shared `Scored`
   envelope; wired into S6 / S14 candidate reranking. Red→green; no golden
   changes. ✅ landed 2026-06-10 (`core/src/closure.rs` — axes + policy +
   `Scored`/`rank_indices` integration; wiring into `generate_structured_set`
   composition remains with the multi-phrase seam increment).
3. **Novelty guard v1**: interval+rhythm n-gram / LCS overlap against the
   corpus manifest; the measure for the `novelty` axis plus a caller-side
   threshold cut. ✅ landed 2026-06-10 (`core/src/novelty.rs` —
   `measure_novelty` over transition sequences; references are passed as
   scores, since the manifest carries no note content).
4. **S14 Phase 3 addition**: burst / rest gesture statistics among the
   persisted chunk axes. ✅ landed 2026-06-11 (`core/src/gesture.rs` —
   `measure_gesture` over the melodic line: burst length distribution,
   rest length + quarter-grid placement, modal-landing share, burst-final
   lengthening; persisted as `ChunkMeta.gesture`, corpus schema v3, and
   filled by `griff curate`).
5. **Parked**: IDyOM-lite expectancy behind S9 data (revisit before S12).

## 8. Sources

Books and classics (no stable open link):

- L. Meyer, *Emotion and Meaning in Music*, 1956.
- E. Narmour, *The Analysis and Cognition of Basic Melodic Structures*, 1990;
  *The Analysis and Cognition of Melodic Complexity*, 1992 — overview:
  <https://en.wikipedia.org/wiki/Implication-Realization>
- E. G. Schellenberg, tests and two-factor simplification of the I-R model,
  *Cognition* 1996 / *Music Perception* 1997.
- C. Krumhansl & E. Kessler, probe-tone key profiles, *Psychological Review*
  89, 1982.
- F. Lerdahl & R. Jackendoff, *A Generative Theory of Tonal Music*, 1983
  (grouping preference rules).
- J. Tenney & L. Polansky, *Temporal Gestalt Perception in Music*, Journal of
  Music Theory 24(2), 1980.
- E. Cambouropoulos, *The Local Boundary Detection Model (LBDM)*, ICMC 2001.
- D. Huron, *The Melodic Arch in Western Folksongs*, Computing in Musicology
  10, 1996; *Sweet Anticipation: Music and the Psychology of Expectation*,
  MIT Press, 2006.
- E. H. Margulis, *On Repeat: How Music Plays the Mind*, Oxford, 2014.
- D. Deutsch, the speech-to-song illusion (repetition turns speech into
  perceived song), JASA 129(4), 2011.

Papers (verified links):

- M. Pearce, *Auditory Expectation: The Information Dynamics of Music
  Perception and Cognition*, Topics in Cognitive Science 4(4), 2012 —
  <https://onlinelibrary.wiley.com/doi/full/10.1111/j.1756-8765.2012.01214.x>
- Hansen et al., *Predictive Uncertainty Underlies Auditory Boundary
  Perception*, 2021 —
  <https://www.marcus-pearce.com/assets/papers/HansenEtAl2021.pdf>
- *Statistical learning and Gestalt-like principles predict melodic
  expectations*, Cognition, 2019 —
  <https://www.sciencedirect.com/science/article/abs/pii/S0010027718303317>
- *Motifs, Phrases, and Beyond: The Modelling of Structure in Symbolic Music
  Generation*, 2024 — <https://arxiv.org/html/2403.07995v1>
- *A Survey on Evaluation Metrics for Music Generation*, 2025 —
  <https://arxiv.org/html/2509.00051v1>
- S. Ji et al., *A Comprehensive Survey on Deep Music Generation* (originality
  / longest-match plagiarism measures in the evaluation section), 2020 —
  <https://arxiv.org/pdf/2011.06801>
- *Fine-Grained Music Plagiarism Detection: Revealing Plagiarists through
  Bipartite Graph Matching*, ACM MM 2023 —
  <https://arxiv.org/abs/2107.09889>
- *MelodySim: Measuring Melody-aware Music Similarity for Plagiarism
  Detection*, 2025 — <https://arxiv.org/abs/2505.20979>
