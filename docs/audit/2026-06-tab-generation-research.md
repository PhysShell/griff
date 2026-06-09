# Tab-generation research — borrow / adapt / reject analysis (2026-06)

Status: research note (not an ADR). Input: an external literature survey of
guitar-tablature generation work (Tuohy & Potter 2005 → Chang et al. 2025),
extended by our own search, evaluated against what griff already has (SPEC,
stages S0…S14, ADR-0012/0013/0015/0017/0018/0019).

The job of this note: say **what to borrow, what to adapt, what to reject for
our specifics** — and pin each borrowable idea to the stage/ADR where it lands.
Nothing here re-opens ratified decisions; items that would need one are marked
*→ candidate ADR*.

## 0. The one-line conclusion

The field converged on exactly the shape griff already chose — *explicit
cost terms over fretboard states, solved by DP/Viterbi, with weights as data* —
before it moved on to transformers. The highest-value borrowings are therefore
not architectural but **calibration and evaluation techniques**: learning our
existing `FingeringWeights` from real tablatures (path-difference learning),
evaluating position inference against Guitar Pro ground truth
(string-assignment accuracy), a worst-transition playability gate (minimax
Viterbi), and a richer inventory of cost terms for the deferred chord-voicing
phase. The neural line of work stays parked behind the S12 gate, as references.

## 1. Where griff already stands (do not re-derive)

| Survey idea | Already in griff |
| --- | --- |
| Fingering as shortest path over (string, fret) states with movement/stretch/string-change/open-string costs (Tuohy fitness; Hori HMM; guitar_dp/tuttut) | ADR-0019; implemented in `core/src/fretboard.rs` (monophonic DP, `FingeringWeights::v1`) |
| Sequence-level optimisation instead of locally-best picks | ADR-0013 (DP/Viterbi as S7 traversal; map vs route) |
| Hard constraints vs soft costs (Bontempi's ILP constraints; Edwards' post-hoc legality fix) | ADR-0017 §5, ADR-0019 §3 (out-of-range = hard reject; costs only rank) |
| User-tunable weights on cost attributes (Bontempi) | ADR-0013 §4 / ADR-0017 §3 / ADR-0019 §4 — weights are data, S9 tunes them |
| Candidate sets + reranking instead of one-shot output (GA populations) | S6 `Vec<GenerationCandidate>` + rerank (ADR-0017) |
| Playability filter on generated material | S6 string/fret playability filter; pair validator (S13) |
| Repetition/structure statistics as a style signal (McVicar's "large amount of repetition") | S14 `StructureMetrics` (self-similarity / autocorrelation), Phase 0 landed |
| Corpus-first, style-by-corpus (not genre tokens) | S5 micro-corpus + swancore tag taxonomy (ADR-0005) |
| "No neural before corpus + baseline" | S12 hard gate (glossary §17.5) |

Takeaway: the survey's "Ideas for Griff" §fitness/§encoding/§operators are
largely a description of ADR-0019 + ADR-0013 with a GA vocabulary. The genotype
"chord = gene" maps to our `EventGroup`; GA fitness terms map to DP cost terms.

## 2. Borrow — high value, fits our specifics

### 2.1 Path-difference learning for `FingeringWeights` (Radisavljevic & Driessen, ICMC 2004)

The single most actionable item. PDL fits DP cost-function weights by gradient
descent on the difference between the DP-optimal path and the path a published
tablature actually took. ADR-0019 §4 already names this as "the natural future
tuner"; the survey confirms the mechanism is published, simple, and designed
for *exactly* our solver shape (DP, linear cost terms).

- **Ground truth we already own:** GP import supplies `Explicit` positions
  (ADR-0018). Every imported GP file is a training tablature; no external
  dataset needed, no licensing exposure.
- **Mechanism:** run the fingering DP on the pitch sequence of a GP file;
  where the optimal path diverges from the explicit path, nudge weights so the
  explicit path gets cheaper (perceptron-style update on the cost difference).
  Deterministic given a fixed corpus order and step size — SPEC §6 holds.
- **Lands at:** `fretboard.rs` `FingeringWeights::v2` as a *named, versioned*
  policy (ADR-0017); aggregate score reproducibility is already defined
  relative to weights-version. Same weight surface S9 later tunes.
- **Effort:** small (a fitting routine + a fixture test). **Risk:** low.

### 2.2 String-assignment accuracy as the position-inference metric (Edwards et al., ISMIR 2024)

Borrow the **evaluation protocol, not the transformer**: measure the % of notes
where inferred `(string, fret)` agrees with the ground-truth tab (they report
~82% next-string accuracy for a large model; DP baselines are the bar to beat).

- **Lands at:** an ADR-0019 acceptance harness — strip `Explicit` positions
  from imported GP fixtures, run inference, count agreement. Gives
  `INFERRED_CONFIDENCE` an empirical basis (today a 0.5 placeholder) and makes
  PDL (§2.1) measurable before/after.
- **Effort:** small. **Risk:** low. Natural red→green test.

### 2.3 Minimax Viterbi as a playability gate (Hori & Sagayama, ISMIR 2016)

Standard Viterbi minimises the *sum* of transition costs, so one impossible
jump can hide inside an otherwise-cheap path. Hori's minimax variant maximises
the *minimum* transition ease — i.e. bounds the worst moment. For us the cheap
adaptation is not a second algorithm but a **worst-transition cap**: track the
max single-transition cost along the DP path; above a threshold, the candidate
fails the playability *gate* (hard reject per ADR-0017 §5) regardless of its
good average.

- **Lands at:** S6 playability filter / S13 pair validator / ADR-0019 DP (one
  extra value threaded through the existing recurrence).
- **Effort:** small. **Risk:** low. Swancore-relevant: fast clean riffs with
  register jumps are exactly where an averaged cost lies.

### 2.4 Cost-term inventory for the chord-voicing phase (Tuohy 2005/2010; Bontempi et al. 2024)

ADR-0019 §7 defers chord voicing and finger assignment. When that phase opens,
the survey provides the vetted term list, so we don't re-invent it:

- **finger span / stretch** within a simultaneous group (Tuohy's "hand
  manipulation"; guitar_dp/tuttut model stretch as finger span, not fret
  distance);
- **barre detection** (one finger reused across strings at one fret);
- **deviation from local fret average** (Tuohy) — a smoother position-shift
  term than pairwise distance;
- **comfortable-neck-region preference** (Bontempi's "distance from
  comfortable fret") — for us doubles as the deferred `timbre_zone`.

**Lands at:** `FingeringWeights` v2+ fields when ADR-0019's chord phase starts.
No action now beyond recording the terms here.

### 2.5 Loop mining from tabs (LooperGP, Adkins/Sarmento/Barthet 2023)

LooperGP's *data preparation* (independent of its transformer) extracts
loopable phrases by finding repeated bookend bars with consistent content
between them. Two uses for us:

- **S14 cross-check:** their loopability criteria (seam smoothness, repeated
  bookends) are an independent formulation to validate our
  `loopability_score` against (S14 Phase 0 known-limitation list).
- **S5 corpus growth:** mine loop candidates from imported GP material to
  propose `PhraseChunk`s, cutting hand-curation cost — an *assistant* to
  `griff curate`, with the human still deciding (active curation, glossary
  §10).

**Effort:** medium. **Risk:** low-medium (heuristic thresholds).

### 2.6 Palm-mute / dead-note as a first-class rhythm state (McVicar et al., AutoRhythmGuitar 2014)

Their 4-state rhythmic encoding treats *muted* events as a distinct rhythm
state rather than a note decoration. For swancore/post-hardcore this is the
right lens: chug patterns are rhythm cells whose identity *is* the
mute/open alternation. We already have `NoteMark::Dead` and palm-mute
`TechniqueSpan`s (ADR-0018) — the borrow is to make the **mute pattern part of
the `RhythmCell` identity** (S5 tags, S6 rhythm-copy strategy, S7 node
features), so rhythm-copy can preserve "which onsets are chugs" and not only
where onsets fall.

**Lands at:** S5 tag taxonomy + S6 `RhythmCopyPitchSubstitute` (carry mute
marks with the rhythm), later S7 `RhythmCell` node features. **Effort:**
medium. **Risk:** low.

## 3. Adapt with caution

- **Transition statistics counted from a corpus** (McVicar n-grams; Hori HMM
  transition probabilities). This is exactly the S7 plan ("transition
  probability counted from corpus"), but the literature is data-hungry; our
  micro-corpus (S5, 20+ chunks) is an order of magnitude below useful n-gram
  density. Keep behind the S7 precondition (≥ ~100 phrases). Do not bring it
  into S6.
- **Articulation placement statistics** (Bontempi: where players put
  hammer-ons/slides/bends, mined from mySongBook). The *generation-side*
  analogue is legitimate for us: when S6/S13 emit techniques, frequencies and
  placements can follow corpus statistics. The *import-side* analogue (guessing
  techniques from plain MIDI) stays parked (decisions.log 2026-06-04, SPEC "not
  a GP-articulation oracle"). Keep the two directions separate.
- **Difficulty/playability rubrics** (chord-ease quantification, rubric-based
  playability scores; e.g. Vélez Vásquez et al. 2025, Pedroza et al.). Borrow
  *features* (stretch, barre, position, finger count) for the S14
  `ComplexityProfile` playability axis — not their trained models, which target
  full songs/chord strumming, not riff-level swancore material.
- **Phrase-level GA-style variation.** GA's real residual value is *diversity
  of candidates*, which we already get from strategy enumeration + seeds +
  rerank. If S6 candidate sets ever feel monocultural, a deterministic
  "mutate one chord-gene / swap two cells" pass (Tuohy's operators re-cast as
  variation rules under a seed) is a cheap S6 strategy addition — an *operator
  vocabulary*, not a GA loop.

## 4. Reject for our specifics (with reasons)

- **GA as the fingering/arrangement solver** (Tuohy 2005/2006/2010). The field
  itself moved on: DP/HMM dominate 2010s, transformers 2020s. For a fixed
  additive cost, DP finds *the* optimum deterministically in linear time —
  a GA is slower, stochastic, and approximate at the same task. ADR-0019
  already chose DP; no GA module.
- **ILP / external solvers** (Bontempi's CPLEX). A heavyweight non-Rust
  dependency to express constraints our DP + hard-gate split already encodes.
  Keep the cost-term inventory (§2.4), drop the mechanism.
- **End-to-end transformer tab models** (Edwards 2024; Fretting-Transformer
  2025; GTR-CTRL / ShredGP / ProgGP / LooperGP-the-model; MoodLoopGP 2024).
  Three independent blockers for now: (1) the S12 hard gate (corpus ≥ ~100
  phrases + S9 feedback loop) is deliberately unmet; (2) determinism under a
  fixed seed is a SPEC rule, and sampling-based generation satisfies it only
  trivially; (3) training data — DadaGP is crowd-sourced Ultimate-Guitar
  material with murky licensing, which our corpus policy (git-ignored, private,
  ADR-0005) exists to avoid; fine-tuning *on top of* DadaGP-pretrained weights
  inherits the same exposure. **Park as the S12 reference list**, where they are
  genuinely the state of the art: Fretting-Transformer / Edwards for
  tokenization of tab-aware MIDI, MIDI-RWKV / MusIAC / Anticipatory Music
  Transformer / MMM for infilling-with-frozen-context (the S11/S12
  region-regeneration shape).
- **Genre/style token conditioning** (GTR-CTRL, ProgGP). Presupposes a neural
  generator. Our style mechanism is swancore-first constraints + S5 tags + S9
  preference weights — same goal, explainable, already specced.
- **Audio-coupled work** (SynthTab, GuitarFlow, audio transcription lines).
  Out of scope by SPEC ("not an audio synthesizer"); only their symbolic side
  (DadaGP-derived annotations) is even adjacent.
- **Whole-piece arrangement** (Hori's arrangement task, McVicar's full-song
  composition). griff's unit is the riff/phrase and the part-pair (S13), not
  song-length arrangement; S14 + S7 cover time-organisation at our scale.

## 5. Datasets — what our search adds to the survey

| Dataset | What it is | Use for griff |
| --- | --- | --- |
| DadaGP (26k GP songs) | Crowd-sourced GP tabs, token format | Not committable (licensing). At most: offline, private statistics (e.g. technique frequencies per genre tag) informing default weights; treat like our private corpus sources. |
| GAPS (ISMIR 2024) | 14h classical guitar, note-level aligned | Research-only license, classical repertoire — wrong idiom; skip. |
| SynthTab (ICASSP 2024) / GOAT (2025) | Synthesized / recorded audio + tab pairs | Audio-side; out of scope. |
| Own GP imports | `Explicit` positions, techniques | **The** ground truth for §2.1/§2.2; already in the import path. |

Side-note from the search: musicology on djent (e.g. Sallings 2021 — riffs as
4–8-bar foundational units; cross-rhythm / rhythmic displacement as the genre
marker) gives vocabulary for S5 tags and supports S14's pattern-period axis
(displaced patterns = period ≠ bar). No MIR paper targets swancore; closest
remains ProgGP's prog-metal fine-tune. Our corpus-first stance is the only
route to the idiom either way.

## 6. Actionable backlog (mapped, ordered)

| # | Item | Lands at | Effort | Risk |
| --- | --- | --- | --- | --- |
| 1 | Position-inference eval harness: agreement vs GP `Explicit` positions (§2.2) | ADR-0019 acceptance; `fretboard` tests | S | low |
| 2 | Path-difference learning → `FingeringWeights::v2` (§2.1) | `fretboard.rs`; weights as versioned data | S–M | low |
| 3 | Worst-transition cap in the fingering DP / playability gate (§2.3) | `fretboard.rs`, S6 filter, S13 validator | S | low |
| 4 | Margin-based confidence (best vs runner-up path) replacing the 0.5 placeholder — falls out of the same DP plumbing as #3 | `fretboard.rs` (ADR-0019 §5) | S | low |
| 5 | Mute-aware `RhythmCell` identity (§2.6) | S5 tags, S6 rhythm-copy | M | low |
| 6 | Loop mining assistant for curation (§2.5) | S5 tooling, S14 metrics cross-check | M | low–med |
| 7 | Chord-voicing cost terms (§2.4) | ADR-0019 deferred phase | M | med |
| 8 | Corpus-counted transition statistics | S7 (behind ≥ ~100-phrase gate) | M | med |
| 9 | S12 reference refresh (Fretting-Transformer, MIDI-RWKV, Anticipatory, MMM) | `S12-neural-assistance.md` when S12 opens | S | — |

Items 1–4 form one coherent increment on the just-landed ADR-0019 module and
are the recommended next step: they convert the survey's strongest content
(cost calibration + evaluation) into measurable improvements on code we already
have, with zero new dependencies and no re-opened decisions.

## 7. Sources

Survey (provided): Tuohy & Potter 2005/2006, Tuohy 2010, McVicar et al. 2014a/b,
Yuan et al. 2020, Kaliakatsos-Papakostas et al. 2022, Bontempi et al. 2024,
Edwards et al. 2024, Sarmento 2024, Chang et al. 2025.

Added by this note's search:

- Radisavljevic & Driessen, *Path Difference Learning for Guitar Fingering
  Problem*, ICMC 2004 — <https://www.ece.uvic.ca/~peterd/papers/PDL_paperICMC2004_ver9.PDF>
- Hori & Sagayama, *Minimax Viterbi Algorithm for HMM-Based Guitar Fingering
  Decision*, ISMIR 2016 — <http://m.mr-pc.org/ismir16/website/articles/285_Paper.pdf>
- Hori et al., *HMM-Based Automatic Arrangement for Guitars with Transposition*,
  ICMC 2014 — <https://quod.lib.umich.edu/i/icmc/bbp2372.2014.193>
- Adkins, Sarmento, Barthet, *LooperGP: A Loopable Sequence Model for Live
  Coding Performance using GuitarPro Tablature*, EvoMUSART 2023 —
  <https://arxiv.org/abs/2303.01665>
- *MoodLoopGP: Emotion-Conditioned Loop Tablature*, 2024 —
  <https://arxiv.org/html/2401.12656v1>
- Sarmento et al., *GTR-CTRL: Instrument and Genre Conditioning for
  Guitar-Focused Music Generation*, 2023 — <https://arxiv.org/abs/2302.05393>
- Sarmento et al., *ShredGP*, 2023 — <https://arxiv.org/html/2307.05324v1>;
  *ProgGP*, 2023 — <https://arxiv.org/html/2307.05328>
- Edwards et al., *MIDI-to-Tab: Guitar Tablature Inference via Masked Language
  Modeling*, ISMIR 2024 — <https://arxiv.org/html/2408.05024>
- *Fretting-Transformer: MIDI to Guitar Tablature*, 2025 —
  <https://www.emergentmind.com/topics/fretting-transformer-model>
- Bontempi et al., *From MIDI to Rich Tablatures: an Automatic Generative
  System incorporating Lead Guitarists' Fingering and Stylistic choices*, 2024 —
  <https://arxiv.org/pdf/2407.09052>
- Riley et al., *GAPS: A Large and Diverse Classical Guitar Dataset*, ISMIR
  2024 — <https://arxiv.org/abs/2408.08653>; companion —
  <https://aim-qmul.github.io/GAPS/>
- *SynthTab*, ICASSP 2024 — <https://synthtab.dev/>; *GOAT*, 2025 —
  <https://arxiv.org/html/2509.22655v1>
- MIDI-RWKV (long-context infilling), 2025 — <https://arxiv.org/html/2506.13001v1>;
  MusIAC, 2022 — <https://arxiv.org/pdf/2202.05528>; Anticipatory Music
  Transformer, 2023 — <https://crfm.stanford.edu/2023/06/16/anticipatory-music-transformer.html>;
  MMM, 2020 — <https://arxiv.org/pdf/2008.06048>
- Sallings, *Change, Longing, and Frustration in Djent-Style Progressive
  Metal* (dissertation, UNT 2021) —
  <https://digital.library.unt.edu/ark:/67531/metadc1808378/>
