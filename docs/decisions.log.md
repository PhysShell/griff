# Decisions log

Append-only Y-statements for small, non-architectural decisions. Format:

> In the context of `<situation>`, facing `<concern>`, we decided for
> `<option>` and against `<alternatives>`, to achieve `<benefit>`, accepting
> `<downside>`.

Architectural decisions go to [`adr/`](adr/) instead.

---

- 2026-05-19 — In the context of bootstrapping the knowledge base, facing a
  Russian constitution but an English-only repo, we decided for a condensed
  English translation of the glossary (all terms preserved, prose tightened)
  and against a verbatim 1:1 translation or keeping Russian, to achieve a
  readable in-repo constitution, accepting that some authorial commentary is
  dropped.

- 2026-05-19 — In the context of mislabeled `feat(sN)` commits, facing a
  conflict with the canonical roadmap, we decided for keeping git history plus
  an audit doc and canonical numbering forward, and against rewriting history,
  to achieve honesty without destabilizing a published branch, accepting that
  old commit messages stay wrong (reconciled in `docs/audit/`).

- 2026-05-19 — In the context of S0 golden CLI tests, facing the `insta` vs
  hand-rolled snapshot-tooling open question, we decided for hand-rolled plain
  golden text files (compared in-process, re-blessed via `GRIFF_BLESS=1`) and
  against the `insta` crate, to keep the workspace dependency tree and the
  strict `cargo-deny`/clippy posture intact, accepting slightly less ergonomic
  snapshot review.

- 2026-05-19 — In the context of S0 `.mid` fixtures, facing the in-repo
  real-MIDI vs synthetic open question, we decided for fully synthetic minimal
  fixtures generated with `midly` (committed, byte-pinned by an in-sync guard
  test) and against any licensed real guitar MIDI, to avoid licensing concerns
  and keep the importer's golden inputs deterministic, accepting that the
  fixtures are not musically realistic.

- 2026-05-31 — In the context of adding ComplementArranger, facing where to slot
  it in the canonical roadmap, we decided for appending it as `S13` (next free
  number, `Depends on: S6`) and against renumbering `S7…S12`, to keep the
  project's append-only posture and avoid renaming six stage files, accepting
  that the integer no longer reflects logical position (captured by the
  dependency note and `docs/audit/2026-05-s13-complementary-arranger.md`).

- 2026-05-31 — In the context of the first ComplementArranger version, facing
  rule-derived vs corpus-mined complement relations, we decided for purely
  generative (derive part B from part A by rule) and against mining real
  two-guitar pairs now, to ship a deterministic baseline without a corpus-schema
  change, accepting that `ChunkMeta` carries no pair relations yet
  (`schema_version` stays 1) and that learning from real pairs is deferred to
  the graph layer.

- 2026-05-31 — In the context of the canon-lift needed for ComplementArranger,
  facing how much of the legacy linear model to retire now, we decided for
  porting only `feature` and `generate` to the canonical model and against a
  full legacy removal up front (ADR-0011), to unblock the new engine cheaply,
  accepting that `classify`/`slice`/the CLI import path stay on the legacy model
  until later characterization-gated ports.

- 2026-05-31 — In the context of S7 traversal, facing weighted-random-walk vs
  dynamic programming, we decided for DP/Viterbi as the primary mechanism (beam
  search only as a large-graph approximation) and against random walk
  (ADR-0013), to get deterministic, whole-sequence-optimal selection that fits
  SPEC §6 without an RNG, accepting that the DP state must stay small and that
  S7 now depends on a realised `EnergyState` and the fretboard model.

- 2026-05-31 — In the context of humanising guitar parts, facing pitch-only
  notes vs string/fret positions, we decided for making the canonical model
  fretboard-aware (`AtomNote` gains an optional `(string, fret)` under the
  score `Tuning`) and against staying pitch-only (ADR-0014), to enable position
  shifts / `fret_jump_penalty` / playability, accepting that position is
  optional (MIDI often can't recover it), inference is a deferred lossy
  sub-problem, and it adds scope to S7.

- 2026-06-01 — In the context of finishing ADR-0011 steps 2–3, facing how to
  move `classify` and the CLI off the legacy `Bar`/`Phrase` types and how
  to present per-bar output once bars are score-level (ADR-0003), we decided for
  a canonical `classify::bar_features_in_range(&Voice, TickRange)` and a
  score-level CLI summary (one `Bars:` line, per-track note counts) — and
  against preserving the old per-track `bars=` column — deliberately re-blessing
  the `import__`/`export__`/`roundtrip__` goldens behind characterization tests,
  accepting a one-off snapshot churn and a marginally smaller `export_score`
  byte stream that still round-trips. With this the legacy linear model is fully
  removed (single internal model).

- 2026-06-01 — In the context of starting S13 (ComplementArranger), facing
  where part-A's profile lives and how much to ship first, we decided for a
  dedicated `PartProfile` in a new `complement` module (over extending the
  feature layer) and a first vertical slice of `rhythm_lock` only — a constraint
  compiler that derives an S6 `RhythmCopyPitchSubstitute` request from A and
  appends B as a new `Track` on A's master bars — plus a minimal `validate_pair`
  and the P2 `complement_request` fuzz target (ADR-0012), accepting that the
  other five relation modes, per-part playability in the validator, and richer
  harmonic context in the profile are deferred to follow-up increments.

- 2026-06-02 — In the context of a structure-controls requirement (separate
  target span / pattern period / repeatability / variation / complexity), facing
  whether it should lean on the planned graph layer (S7) or DP/Viterbi
  (ADR-0013), we decided for a self-contained structure layer — a constraint
  compiler over S6 plus a self-similarity/autocorrelation metric pass — that
  depends on neither, and against pulling S7/DP in early (ADR-0015, new stage
  S14), to keep the dependency running one way: `StructureMetrics` are designed
  to *become* S7 node attributes and DP transition-cost features later, never to
  consume them. Accepted: complexity stays a vector (not a scalar), `phrase_length`
  is reused from S4 rather than re-added, and a `ChunkMeta` schema bump is
  deferred to the corpus phase.

- 2026-06-03 — In the context of the Codex review of PR #18 (the viewport
  refactor, ADR-0016), facing three pre-existing P2 issues in the S8 slice-2
  code (phantom right-edge note, `fit` floor division clipping the tail,
  bar classification reading only the first voice), we decided to keep PR #18 a
  clean behaviour-preserving refactor and record the findings in
  [`audit/2026-06-preview-known-issues.md`](audit/2026-06-preview-known-issues.md)
  as deferred follow-ups, and against folding the fixes in (two change the
  golden frames; the third is a semantic analysis-layer decision), to keep the
  refactor's "no behaviour change" contract intact and revisitable.

- 2026-06-03 — In the context of the note still being pianoroll-shaped (a single
  `Option<Articulation>`, no string/fret), facing whether to fix techniques and
  fretboard position separately, we decided for one merged core-model migration
  (ADR-0018, superseding ADR-0014) — a note gains an optional `FretboardPosition`
  under a per-`Track` `Tuning`, a *set* of `NoteMark`s (replacing the single
  `Option`), enriched `TechniqueSpan`s with a `SpanTechnique` kind, and
  `TechniqueEvidence` (`Explicit` vs `InferredFromMidi` + confidence) on every
  technique/position — and against two separate ADRs, because both live on the
  same note/group and must migrate together (one shape change, one golden
  re-bless). Accepted: this ships no code (phased — model+projection, then GP/MIDI
  population, then playability/DP consumption, each characterization-gated);
  `Articulation` survives only as a compatibility projection; position inference
  and `NoteId` endpoint pinning stay deferred sub-problems; a corpus
  `schema_version` bump is deferred to persistence.

- 2026-06-03 — In the context of the same scoring-with-provenance shape recurring
  across the canon (boundary `score`/`BoundaryReason`/`weights`, complement
  `AxisScores`, the DP cost terms of ADR-0013, the `ComplexityProfile` of
  ADR-0015, the flat "Quality score"), facing five bespoke shapes that cannot
  share a tuning surface, a UI, or a vocabulary, we decided for one scoring
  contract — axes (data) + weights (S9-tunable policy) + rationale + a *derived*
  aggregate, carried in a shared `Scored<T>` (ADR-0017) — and against letting
  each consumer mint its own type, to give S9 a single weight surface and one
  "why this candidate" inspector, and to design out the scalar-quality and
  evidence/rationale-collision hazards. Accepted: this ships no code (the
  complement+boundary migration is a later characterization-gated slice), every
  stored score must carry a weights-version to stay reproducible under feedback
  (a deferred `schema_version` bump), and the contract is only proven when a
  second consumer (DP cost or S9) reuses it.

- 2026-06-03 — In the context of S14 structure metrics being diluted by a
  trailing empty bar, facing where to cut the sentinel, we decided to fix it at
  the source — `midi::build_master_bars` now loops `while bar_start < end_tick ||
  master_bars.is_empty()` instead of `<=`, so content ending exactly on a barline
  no longer appends an empty bar — and against masking it downstream in the
  metrics, to keep one honest master timeline (the bar holding the last event is
  still always built; an empty score still yields one bar). Accepted: this is an
  intentional behaviour change that re-blessed the import / inspect / classify /
  roundtrip / characterize goldens for all four fixtures (each was one content
  bar plus the sentinel) behind a red unit test pinning the new bar-count
  contract; no notes are lost and roundtrip still re-imports identically.

- 2026-06-03 — In the context of resolving preview P2 #3 (bar classification
  read only `voices.first()`, so material in voice 2+ was invisible to the
  section bands), facing whether to merge every voice's atoms into one
  `BarFeatures` before classifying or to classify each voice and reduce, we
  decided for **merge-then-classify** — a new `bar_features_across_voices` in
  `griff_core::classify` aggregates note count / average velocity / pitch span
  across all voices of the focus track, with `bar_features_in_range` kept as a
  thin single-voice wrapper over it — and against a per-voice-then-reduce scheme
  and against the original fix's duplicate aggregator living in the preview
  layer, to give multi-voice imported parts (e.g. one `Voice` per Guitar Pro
  voice) a single honest classification and keep the feature math in core (one
  home, no drift). Accepted: this is the coarse named-section heuristic, not the
  S14 structure metrics; if per-voice section semantics are ever needed this is
  revisitable.

- 2026-06-04 — In the context of ADR-0018's deferred MIDI inference, facing that
  MIDI articulations/slides are virtual-instrument-specific (libraries encode
  them as keyswitches on out-of-range low notes, mapped differently per VI) and
  so are not reliably recoverable from plain MIDI, we decided to **park MIDI
  technique inference** — Guitar Pro stays the source of truth for techniques,
  and MIDI contributes notes, velocity, and timing only — and against building a
  MIDI-articulation guesser (reaffirming glossary §17.3/§19 and SPEC's
  "not a GP-articulation oracle"), accepting that
  `TechniqueSource::InferredFromMidi` / `confidence` may stay unused for
  techniques until a reliable signal exists (the field is cheap and stays for
  that day). Distinct and **not** parked: position inference (pitch → string/fret)
  is a VI-independent fretboard-geometry problem under the score `Tuning`, valid
  but low-priority — it only matters for playability / `fret_jump_penalty` on
  MIDI-sourced material. (Per-VI keyswitch notes are also import noise we do not
  decode; out of scope.)

- 2026-06-09 — In the context of preparing S14 Phase 1 (the tile/vary
  compiler needs a motif-identity measure, and Phase 2 reranks by metric
  distance), facing that exact-pitch bar signatures read any pitch-varied
  repeat — including the S6 pitch-substitute and motif-transpose strategies'
  output — as no repeat at all (the documented Phase-0 "transposed repeats"
  limitation, which would make the structure metrics a blind referee for the
  very compiler they are meant to grade), we decided for **contour-aware bar
  similarity** in `structure::detect_period` — a weighted sum of onset-grid
  Jaccard (`RHYTHM_WEIGHT`) and exact `(onset, pitch)` Jaccard, where a
  transposed repeat (identical rhythm, constant non-zero interval shift,
  chord-safe via sorted positional alignment) lifts the pitch component to
  `TRANSPOSITION_CREDIT` — so verbatim tiles read high, transposed tiles
  medium, rhythm-only tiles partial, unrelated material low — and against a
  full interval-contour signature (overkill before sub-bar periods exist) and
  against changing `structural_complexity` (it deliberately keeps counting
  exact distinct signatures: a transposed sequence is genuinely more material
  than a verbatim loop). Accepted: this is an intended behaviour change to
  `repeatability_score` / `variation_score` / period detection behind red
  tests (the old through-composed fixture — a chromatic sequence over a
  constant rhythm — was itself a transposed repeat and was re-fixtured); the
  two weights are compile-time constants for now and graduate to data
  (ADR-0017) when a consumer needs to tune them.
- 2026-06-05 — In the context of where griff sits relative to formats, facing
  whether MIDI is the engine's orientation, we decided that **Guitar Pro /
  tablature is the primary, source-of-truth import format** (strings, frets,
  techniques, tuning) and **MIDI is a lossy interchange adapter** (pitch /
  velocity / timing only) — and against treating MIDI as the primary or defining
  format — to match the rich note model (ADR-0018/0019): everything that model
  wants, GP gives directly, while MIDI must be inferred (positions, ADR-0019) or
  parked (techniques, the keyswitch decision). The canonical model stays the
  internal truth (hard rule #1); this is about *import* only. Accepted: MIDI
  import remains, just demoted; the CLAP MIDI-out delivery target (ADR-0007 / S10)
  is unchanged here.

- 2026-06-05 — *(Unresolved future direction, not a decision.)* The inverse of the
  parked MIDI technique-inference is promising on **export**: because griff holds
  rich techniques internally (from GP), it could emit **technique-aware MIDI
  against a chosen instrument's articulation profile** (technique → keyswitch /
  CC / channel), automating the manual "fix articulations per VI in the DAW
  piano-roll" chore. The VI-specificity that kills *import* inference is
  manageable on *export* because the target is chosen. Constraints: per-VI
  articulation-map profiles are data to design; MIDI cannot express everything
  (continuous/polyphonic bends need pitch-bend automation / MPE; some timbres not
  at all), so export stays **lossy-with-LossReport**. Captured so it is not lost;
  no commitment to build.

- 2026-06-05 — In the context of how to choose between building and reusing, we
  adopted a **prior-art-first** workflow rule in `AGENTS.md`: search for existing
  solutions before inventing, reuse the *idea* by default and *code* only when
  licence- and dependency-posture-compatible (so usually a native
  reimplementation, not a new crate). Reaffirms the lean-dependency posture
  (the `insta` rejection) and records the practice that worked for ADR-0019.

- 2026-06-10 — In the context of S14 Phase 1 (the tile/vary compiler over S6,
  ADR-0015 §4), facing how a varied copy should mutate while staying "the same
  motif" — and how the two control knobs divide the work — we decided for
  **two-level seed-deterministic gating with transposition as the only
  variation operator**: `repeatability` is the per-copy probability of a
  verbatim repeat (copy 0 always verbatim), `variation_rate` is the per-bar
  mutation probability inside a varied copy, and a mutated bar is transposed
  by a per-copy interval from a fixed list (`±3/±5/±7`, seed-offset cyclic so
  adjacent and lag-2 copies never coincide) with rhythm preserved — exactly
  the operator the contour-aware bar similarity (2026-06-09) reads as a
  medium repeat, so the compiler and its referee agree by construction — and
  against pitch-substitute mutation (reads as rhythm-only repeat, weaker
  identity) and against per-bar random intervals (adjacent copies could
  coincide and shift the detected period to an accidental multiple).
  Decisions in the same increment: `pattern_period_bars = None` delegates to
  plain S6 (through-composed), a truncated final copy restarts from the
  base's first bar, a transposed note drops any carried fretboard position,
  and `StructuredCandidate` returns measured `StructureMetrics` as provenance
  (Phase 2 reranks by control↔metrics distance). Accepted: the P2
  `structured_request` fuzz target is deferred (no nightly toolchain in the
  landing environment) and is named in the stage doc as remaining work;
  variation strength (interval magnitude scaling) and loopability targets
  stay future increments.

- 2026-06-10 — In the context of S14 Phase 2 (reject / rerank structure
  candidates by metric distance, ADR-0015), facing how "distance between what
  was asked and what was produced" should be represented, we decided for
  **agreement axes under the shared ADR-0017 vocabulary** — `period_match`
  (equal periods or both through-composed = 1.0, `min/max` ratio for a wrong
  period, 0.0 across the periodic/aperiodic divide), `repeatability_match`
  and `variation_match` (`1 − |requested − measured|`) — scored by a uniform
  `structure` v1 `WeightPolicy`, wrapped per candidate in the `Scored`
  envelope (value = candidate seed; provenance = seed + policy version), and
  ranked by the existing `rank_indices` tie-break; `generate_structured_set`
  derives per-candidate seeds via the SplitMix mix (candidate 0 keeps the
  request seed, so the set extends the single pass) — and against a bespoke
  distance scalar (the anti-scalar rule, ADR-0017 §2) and against a built-in
  rejection threshold (rejection is the caller's cut on the aggregate; the
  threshold is a future tunable, not code). Accepted: the repeatability /
  variation knobs and their measured scores are different quantities (per-copy
  / per-bar probabilities vs mean self-similarity); the absolute distance is
  documented as the honest v1 proxy, and the weight surface is where S9
  recalibrates later. A loopability axis is deferred until the control carries
  a loopability target.

- 2026-06-10 — In the context of surveying NeptuneHub/AudioMuse-AI (AGPL-3.0;
  self-hosted *audio* retrieval: Voyager ANN similarity, radius-walk ordering,
  song-path interpolation, ADD/SUBTRACT "alchemy" centroids, evolutionary
  clustering, 2D music map, recency-weighted sonic fingerprint) as prior art
  for the retrieval / corpus-exploration layer (high relevance: S5/S7/S9/S14),
  facing which of its mechanisms to adopt without scope creep, we decided for
  **ideas only — no code, no dependency** (AGPL-3.0 is incompatible with this
  MIT crate; per the AGENTS.md prior-art rule), adopting four shapes mapped
  onto existing canon: (a) chunk similarity as the first S7 slice —
  brute-force cosine over *named* symbolic feature axes with a per-axis
  rationale (ADR-0017; no ANN index at micro-corpus scale); (b) ADD/SUBTRACT
  alchemy as a deterministic add/avoid-centroid rerank under a versioned
  `WeightPolicy` — the query-time complement of the S9 profile (whose EMA
  update already *is* the fingerprint's exponential recency decay); (c)
  feature-space interpolation as a transition *constraint schedule* compiled
  onto S6 (the ADR-0012/0015 compiler pattern) — interpolate density /
  register / dissonance / period targets per bar, never audio vectors; (d) a
  corpus map as a curation dev-tool, gated on S14 Phase 3 persisting numeric
  axes into `ChunkMeta` and on corpus scale (~50+ chunks) — scatter over two
  named axes or a natively implemented 2-component PCA, SVG export from CLI
  tooling, no UMAP dependency, never a runtime path — and against adopting its
  greedy radius walk (it re-states the locally-best traversal ADR-0013 already
  rejected; only its 70/30 prev/anchor balance survives, as calibration input
  for the `phrase_continuity` vs `style_fit` cost terms), against whole-track
  audio embeddings as core features (an unexplainable scalar — the anti-scalar
  rule, ADR-0017 §2), against ANN / media-server machinery, and against
  evolutionary generator-config tuning now (parked S9-late at the earliest;
  reproducibility would ride on ADR-0017 policy versioning), to achieve a
  production-validated confirmation of the planned retrieval shape instead of
  new architecture, accepting that all adopted ideas wait on their gates (S14
  Phase 3 schema bump, S9 feedback logging, corpus growth) and none lands
  today.

- 2026-06-10 — In the context of the same survey raising "audio in, vibe out"
  — extract the vibe of a real song and turn it into generation parameters —
  facing audio analysis in core vs symbolic-first, we decided for
  **reference-as-intent staying symbolic and in-core**: the
  profile-extractor primitive of
  `audit/2026-06-expressive-control-and-scoring.md` §2.4 (consumer 4 — a
  profile extracted from a reference phrase / selected region *is* a
  generation intent, "formalisation by example") over GP/MIDI references
  through the normal import path, with **audio entering only via an
  out-of-workspace transcription sidecar** (audio → GP/MIDI → import → axes →
  constraints); the derived intent must mark which axes the lossy path
  supports — tempo, density, syncopation, register, contour, structure period
  survive; techniques do not (no articulation oracle from plain MIDI, SPEC /
  glossary §17.3; loss-report mindset, SPEC hard rule 7) — and against an
  in-workspace audio stack (librosa-equivalent DSP / CLAP-style audio-text
  embedding dependencies, even optional), to achieve "vibe from a reference"
  without breaking the audio boundary or the lean dependency tree while
  reusing the extractor shared with `PartProfile` / `StructureMetrics`,
  accepting that audio-only references depend on external transcription
  quality and carry no technique evidence.

- 2026-06-10 — In the context of the "nonsense generator" concern (DGD-style
  burst-and-rest writing risking three-notes-plus-awkward-rests output that
  feels broken off), facing how to make "this melody feels finished"
  operational without mining verbatim phrases from real songs, we decided for
  recording the melodic-closure survey as
  `audit/2026-06-melodic-closure-research.md` and adopting its backlog — a
  rule-based **closure axis** under the ADR-0017 vocabulary (the S4 boundary
  detector re-used as the generation-side referee, plus ending-stability
  (Krumhansl–Kessler) and gap-fill/reversal (Narmour) components) and a
  **novelty guard** (interval+rhythm n-gram / LCS overlap cap against the
  corpus) as the concrete measure for the glossary's `novelty` axis — and
  against a trained closure classifier now (ADR-0008 / S12 gate) and against
  verbatim corpus-pattern reuse as a musicality guarantee, to achieve
  explainable completeness scoring grounded in the closure literature
  (expectation realization, tonal stability, phrase-final lengthening,
  recurrence-makes-intention) while keeping corpus learning at the schema
  level (distributions, not note content), accepting that the closure axis
  and the guard land as backlog (no code in this increment) and that
  swancore-specific weights await S5 corpus calibration and S9 feedback.

- 2026-06-10 — In the context of landing closure axis v1
  (`core/src/closure.rs`, melodic-closure note §7.2), facing how each axis
  should be made concrete, we decided for: a referee `BoundaryConfig` derived
  from the score's own PPQN (snap 1/16, min-gap two quarters — the S4
  defaults assume PPQN 960) with default weights/threshold;
  `internal_continuity = 1 − strongest boundary strictly inside (first
  onset, last note end)`; a simplified Krumhansl-inspired stability tier
  table (root 1.0 unconditional; in-material fifth 0.8 / third 0.7 / other
  0.5; outside 0.2) with a landing chord taking its most stable note;
  `final_lengthening = landing duration / (2 × mean)` clamped to `[0, 1]`
  (equal-to-mean reads 0.5); gap-fill tiers over the
  highest-pitch-per-onset line (unresolved final leap > 7 st → 0.0;
  ≥ 5 st leap answered by a smaller opposite interval → 1.0; stepwise ≤ 2 st
  → 0.8; else 0.4; fewer than two line notes → 0.5 neutral); and the uniform
  `closure` v1 `WeightPolicy` — and against porting Krumhansl–Kessler
  profile values verbatim (false precision over an arbitrary
  `PitchMaterial`), against audio-side cues, and against a multi-phrase
  seam-aware referee now (the track is treated as a single phrase; S14-tile
  composition is the next increment). Accepted: the S4 hard-rest rule makes
  a long mid-phrase hole collapse continuity to exactly 0.0 (the red suite
  expected a partial break; the bound was relaxed to `< 0.5` in the green
  step with the reason documented inline), and the tier/tier-constant
  choices are v1 placeholders the S5 corpus and S9 feedback recalibrate.

- 2026-06-10 — In the context of S14 Phase 3 (persist measured structure into
  the corpus schema), facing how a schema bump should treat existing v1
  records, we decided for an optional `ChunkMeta.structure:
  Option<StructureMetrics>` — `serde(default)` on read,
  `skip_serializing_if` on write (the key is absent, never `null`), so v1
  records load as `None` and round-trip byte-identically — plus a
  `SCHEMA_VERSION = 2` constant, serde derives on `StructureMetrics` itself,
  and `griff curate` measuring the *first note-bearing track* — and against
  a required field with a forced migration pass (the corpus is git-ignored
  and tiny; a rewrite buys nothing), against a parallel serialisable
  metrics DTO (drift risk against the analysis type), and against per-track
  metric lists (chunks are single-part by S5 convention). Accepted: a v1
  record reads as unmeasured until re-curated, multi-track chunks carry only
  their first note-bearing track's metrics, and the gates this opens
  (similarity / alchemy rerank / corpus map, decisions 2026-06-10 AudioMuse
  entry) still wait on corpus scale.

- 2026-06-10 — In the context of landing the novelty guard v1
  (`core/src/novelty.rs`, melodic-closure note §7.3), facing what
  representation makes a verbatim quote detectable, we decided for
  **transition sequences** — `(pitch interval, IOI rescaled to a 480-per-
  quarter grid)` between successive notes of the highest-pitch-per-onset
  line (the closure/curate conventions) — so quotes survive transposition
  and PPQN changes; references enter as `&[Score]` reading each one's first
  note-bearing track (the manifest carries no note content); the longest
  common run uses a direct O(n·m·len) scan with ties going to the first
  reference; n-grams are 4 transitions (≈ a five-note figure) in a
  `BTreeSet`; axes are computed as single correctly-rounded *free-share*
  divisions (`(total−taken)/total`, not `1 − taken/total`) so exact shares
  compare equal to literals — and against absolute-pitch or duration-exact
  matching (transposition/notation would hide quotes), against a built-in
  rejection threshold (the caller cuts on `NoveltyReport`, per ADR-0017
  spirit), and against a suffix automaton now (parked until corpus scale
  demands it). Accepted: rhythm matching is onset-based (note durations are
  ignored), sub-grid IOI remainders truncate, and a chord participates only
  through its top note.

- 2026-06-11 — In the context of growing the S5 corpus toward the S7
  threshold (~100 phrases), facing whether adjacent-genre material
  (Underoath, Hopesfall, …) enriches a swancore-first corpus or dilutes it,
  we decided for **admitting adjacent-genre chunks under an explicit cohort
  label, consumed through per-consumer slices**: corpus-derived *statistical
  gates* (the S6 acceptance bands — density within corpus mean ± 1σ,
  syncopation ≥ lower quartile) and the future style centroid (style as a
  *region* over idiom axes, audit 2026-06 §2.3) read the **core swancore
  slice only**; the graph layer (S7) reads the full corpus for nodes,
  similarity edges, and recombination material, but counts transition
  statistics **per cohort** and blends them by an explicit weight (a future
  user-facing knob — cross-genre grammar is a control, never an accident);
  novelty-guard references and S9 taste ignore cohorts entirely (more
  references only strengthen the guard); target mix ≈ 70–80 % core /
  20–30 % adjacent, validated empirically on the corpus map (each adjacent
  chunk is either in-region coverage or a deliberate outlier — judged by
  distance to the core centroid, not by genre prejudice) — and against
  unlabeled mixing (silently shifts the mean ± σ the S6 acceptance reads and
  blurs the style centroid) and against excluding adjacent genres outright
  (swancore overlaps them on many idiom axes, so their in-region chunks are
  coverage, and S7 connectivity needs the mass). Accepted: `ChunkMeta` v2
  carries no cohort field yet — a `style_cohort` (or band/provenance) field
  is the next schema increment (v3, the same optional-field migration
  pattern as v2); until it lands, the cohort convention lives in chunk ids /
  titles (e.g. `uo_` / `hf_` prefixes), which is fragile and explicitly
  short-term.
- 2026-06-10 — In the context of landing chunk similarity v1
  (`core/src/similarity.rs`) — the first S7 edge, implementing idea (a) of
  the AudioMuse prior-art entry above — facing what the edge should measure
  at micro-corpus scale, we decided for per-axis *agreement* facts under
  the ADR-0017 envelope, computed only from facts already persisted in
  `ChunkMeta` (schema-v2 `StructureMetrics` + tags, no note content):
  `period_similarity` as the min/max ratio of detected bar periods
  (through-composed pairs agree at 1.0; one-sided periodicity scores 0.0),
  `1 − |Δ|` on the repeatability / loopability / structural-complexity
  scalars, and Jaccard over tag sets (untagged pairs agree) — ranked by
  `rank_indices` under the uniform `similarity` v1 `WeightPolicy`, with an
  unmeasured query a typed error and unmeasured candidates skipped until
  re-curated — and against a literal cosine over the raw feature vector
  (the AudioMuse entry's shorthand: over bounded non-negative axes cosine
  degenerates toward 1, and its joint normalisation does not decompose
  into the per-axis `value·weight` rationale ADR-0017 requires), against a
  `variation_similarity` axis (`variation = 1 − repeatability` by
  construction; a duplicate axis would silently double-weight one fact),
  against tempo / register / technique axes now (not yet persisted as
  comparable numerics; later increments alongside richer corpus features),
  and against any ANN index (brute force is exact, explainable, and cheap
  at this scale), to achieve an inspectable first retrieval edge over the
  axes S14 Phase 3 just persisted, accepting that the edge is blind to
  note content (two chunks with equal metrics and tags read as identical),
  that period similarity compares bar counts only (tick-resolution and
  meter differences are invisible), and that the uniform weights await S9
  tuning.

- 2026-06-11 — In the context of completing the five remaining S13 relation
  modes (`core/src/complement.rs`, each its own red→green increment per the
  stage doc), facing how each mode's contract should be made concrete on the
  existing skeleton, we decided for: `octave_double` as a verbatim
  contour copy (onsets, durations, velocities, marks) shifted by a
  `register_offset` that must be a non-zero whole octave (typed
  `InvalidSpec` otherwise — a third-doubling is a different relation);
  `register_contrast` as the rhythm-lock grid in A's band shifted by the
  offset, rejected as `InvalidSpec` when the shifted band still intersects
  A's after MIDI clamping (including a clamp folding it back onto A);
  `support_layer` as one root pedal per non-empty bar — A's first onset,
  that note's duration and velocity, A's lowest pitch shifted — so the
  layer is strictly sparser wherever A plays more than one note a bar;
  `call_response` answering each ≥-one-quarter gap of A's *merged* note
  coverage (between A's first sound and the span end) with one note
  sustaining through the gap at the preceding call's velocity, leading
  silence unanswered, and a gapless A the typed `NoGapsToAnswer`;
  `counter_melody` as the one true S6 delegation — `ConstrainedRandomWalk`
  over a request derived from A (pitch classes as scale, shifted band as
  bounds, A's meter/tempo/PPQN/bar count, bar rhythms as templates) lifted
  onto A's master bars, with a mid-score meter or bar-span change the
  typed `NonUniformTimeline` (S6 lays bars back-to-back from one meter);
  plus generalising `rhythm_similarity` provenance from a hardcoded `1.0`
  to the onset-set Jaccard between A and B (grid-locked modes still
  measure exactly 1.0) and removing the now-unreachable
  `ModeNotImplemented` variant — and against fixed mode defaults hidden in
  code (e.g. snapping a stray offset to an octave), against answering
  leading silence (an answer needs a call), against a B-empty fallback for
  gapless or non-uniform inputs (silent degradation over a typed error),
  and against routing the grid-locked modes through an S6 round-trip (A's
  onsets already respect A's timeline; regeneration could only misalign).
  Accepted: `support_layer` equals A's density when A is one-note-per-bar,
  `call_response` ignores velocity decay across long gaps, the
  `counter_melody` rhythm comes from the S6 walk rather than a
  complement-aware rhythm model (S7's cost terms take over there), and
  removing `ModeNotImplemented` is a breaking enum change inside the
  pre-1.0 workspace.

- 2026-06-11 — In the context of landing burst/rest gesture statistics
  (`core/src/gesture.rs`, melodic-closure note §3.5/§7.4) as persisted
  chunk axes (corpus schema v3), facing what to measure and how to segment
  a gesture, we decided for **distributions, not content**: burst
  count/mean/max over maximal melodic-line runs, with a *gesture rest*
  defined as at least one quarter of line silence after a sounded note
  (the trailing gap to the span end included, leading silence excluded —
  a rest belongs to the gesture before it; sub-quarter holes are
  phrasing); rest placement as the share of rests starting on the quarter
  grid of their bar (the §1.3 metrical-predictability cue); landing degree
  as the share of bursts ending on the line's modal pitch class (ties to
  the smallest class — a key-free root proxy until `PartProfile` grows
  harmonic context); burst-final lengthening under the closure-v1
  normalisation (`landing / (2 × mean)`, clamped, equal-to-mean reads
  0.5); the highest-pitch-per-onset line convention shared with
  closure / novelty / curate; the Phase-3 serde pattern for persistence
  (`SCHEMA_VERSION = 3`, absent key on older records, byte-identical
  v1/v2 round-trips); and `griff curate` measuring the same first
  note-bearing track it already measures structure on — and against
  persisting raw histograms (compact scalars serialize stably and suffice
  at micro-corpus scale), against a key-aware landing degree now
  (`ChunkMeta` carries no key; verbatim Krumhansl porting was already
  rejected in closure v1), against an eighth-based rest threshold
  (syncopated eighth holes inside a flurry are phrasing, not rests), and
  against making the stats `Scored` axes now (they are corpus facts and
  future S6 constraint inputs; a similarity / scoring join is a
  follow-up). Accepted: the grid check uses the quarter grid only
  (denominator-aware beat grids deferred), the modal class is a crude
  root proxy, vacuous shares read as predictable (`1.0`) and absent rests
  as zero length, and a chord participates only through its top note.

- 2026-06-11 — In the context of joining the persisted gesture statistics
  to the chunk-similarity edge (`core/src/similarity.rs`, the follow-up
  the gesture-v1 entry above explicitly deferred), facing which gesture
  facts may become similarity axes and what an unmeasured side now means,
  we decided for five new agreement axes over the *intensive*
  distributions only — `burst_length_similarity` and
  `rest_length_similarity` as min/max ratios (the period-axis convention
  on a continuous fact: two restless chunks agree at 1.0, wall-to-wall
  against gestured writing reads 0.0), `rest_grid_similarity`,
  `modal_landing_similarity`, and `final_lengthening_similarity` as
  `1 − |Δ|` on the unit shares — appended after the five v1 axes under a
  uniform `similarity` v2 policy, with *measured* tightened to structure
  **and** gesture (an unmeasured query stays the typed `QueryUnmeasured`;
  v1/v2 candidates are skipped until re-curated, which `griff curate` now
  heals in one pass) — and against axes over the extensive facts
  (note / burst / rest counts, max burst scale with chunk length; a
  length echo would shadow the style facts, the same double-weighting
  argument that excluded `variation_score` in v1), against partial axis
  sets for gesture-less pairs (aggregates over different axis counts are
  not comparable under one policy, and an absent fact is not a
  zero-similarity fact), and against keeping a `similarity_weights_v1`
  constructor alongside v2 (superseded weights are data in git history,
  not API surface; nothing persists rankings yet), to achieve a similarity
  edge that hears burst-and-rest writing, accepting that re-tightening
  "measured" temporarily shrinks the edge until v2 records are re-curated
  and that the uniform v2 weights await S9 tuning.

- 2026-06-11 — In the context of making the gesture statistics actual S6
  constraint inputs (`core/src/gesture.rs`, the destination the
  melodic-closure note §3.5 and the gesture-v1 entry assigned them),
  facing how a target distribution should constrain a generator whose
  strategies write wall-to-wall, we decided for a **constraint compiler
  over the S6 generator** (the ComplementArranger / StructureControl
  pattern, ADR-0012/0015), not a request-struct change: `GestureControl`
  (`burst_notes`, `rest_quarters`) is the *ask* counterpart of the
  measured `GestureStats`, derivable from a corpus chunk via
  `from_stats` (burst mean rounded with a floor of 1; rest mean clamped
  to the one-quarter gesture floor — a restless chunk derives the
  minimal gesture, callers wanting wall-to-wall skip the compiler);
  `generate_gestured` runs the plain S6 pass, then carves
  deterministically — after every `burst_notes` kept notes it drops
  following notes until at least `rest_quarters` of line silence opens —
  and returns the score with its re-measured `GestureStats` as
  provenance (ask vs is); invalid controls are the typed
  `InvalidControl`; a `gesture_request` fuzz target (P2, ADR-0010) pins
  no-panic, the untouched master timeline, carve-only-removes (every
  survivor is an unmoved plain-S6 note), provenance-equals-remeasure,
  and seed determinism — and against extending `RuleGenerationRequest`
  with an optional gesture field (every construction site incl. the fuzz
  crate breaks for a concern that composes cleanly on top; the sibling
  compilers already set the layering), against carving inside the
  strategy loop (strategies stay gesture-blind; the carve is one
  inspectable pass), against an RNG in the carve (the only randomness
  stays the seeded S6 PRNG), and against padding the trailing rest (the
  carve does not invent material; a short final burst is honest output),
  accepting that carved rest lengths quantise to the dropped notes'
  durations (rests can overshoot the target, and a trailing carve can
  undershoot it), that grid alignment of rests follows the strategy's
  rhythm rather than being enforced, and that the carve assumes the S6
  output shape (back-to-back single-note groups, which `generate`
  guarantees).

- 2026-06-11 — In the context of curating DGD-style two-guitar material
  (left/right-channel guitars with **no stable rhythm/lead split** — the
  roles swap per phrase), facing how the corpus should record that several
  tracks of one source span are *one ensemble phrase* so the graph layer can
  later mine real complement relations (the hook ADR-0012 §3 deliberately
  left open: "may require a schema bump then"), we decided for **ensemble
  groups over single-part chunks**: every member stays an ordinary
  single-part chunk (preserving the chunk = one-part convention that the
  v2/v3 metrics and similarity v1/v2 are built on); an optional per-chunk
  `ensemble` link (`group_id`, `part_index`) plus a manifest-level group
  record carrying member ids and the **measured pairwise relation axes**
  (rhythm similarity / register overlap / density ratio / technique overlap
  — computed at curation time with the existing `PartProfile` /
  `AxisScores` machinery; the same measure-at-curate pattern as schema
  v2/v3); `griff curate` offers ensemble curation when a span has ≥ 2
  note-bearing tracks; more than two parts are pairwise axes — the glossary
  §9 complement *hyperedge* made concrete — and against role labels on
  tracks ("rhythm" / "lead" are not stable roles in the target idiom; the
  per-phrase relation axes *are* the role information, and role fluidity
  becomes data — the same pair of guitars measures near-unison in one chunk
  and call-response in the next), against multi-part chunks (breaks the
  single-part convention and every consumer built on it), and against
  deferring the link until S7 consumes it (chunks curated without links
  would need re-curation — the link must be recorded from the first DGD
  curation session, so this lands **before mass curation**). Accepted: this
  entry records the design direction, not code — it lands as a future
  schema bump through the usual red→green cycle; gesture stats have
  meanwhile taken schema v3, so this and the style-cohort field (2026-06-11
  entry above) are v4 candidates, plausibly one combined bump; and the
  persisted pair axes duplicate what S7 could recompute from members
  (stored anyway as provenance, for mining speed and curation-time
  inspection).

- 2026-06-11 — In the context of implementing corpus schema v4 (the cohort
  and ensemble direction entries above, realised as one combined bump),
  facing the remaining implementation choices, we decided for: a
  `StyleCohort` enum (`core` / `adjacent`, `None` = unlabeled pre-v4
  record) and an `EnsembleRef { group_id, part_index }` on `ChunkMeta`,
  plus manifest-level `EnsembleGroup` records whose `PairRelation`s persist
  `AxisScores` measured by the new `complement::measure_pair_axes`
  (built on `analyze_part` + the shared band/Jaccard helpers;
  `PartHasNoNotes` on empty parts; `density_ratio` oriented *b relative to
  a*, lower part index first); `griff curate` gains a cohort prompt after
  tuning (blank = core, so EOF-driven scripts keep working) and an
  `--ensemble` mode writing `<stem>.p<N>.chunk.json` per note-bearing
  track plus `<stem>.group.json`, with shared tags/flags across parts as
  the v1 simplification (records are editable text) — and against
  per-part interactive prompts (seven prompts × N parts is curation
  hostility), against role labels anywhere in the flow, and against
  weakening the failing test to approximate float equality. That failing
  test exposed a real defect: **serde_json's default fast float parser can
  be one ULP off** (the multi-track fixture's 11/12 loopability parsed
  back unequal to the written value), silently breaking the schema's
  lossless-roundtrip promise for real measured values — the earlier
  property tests missed it by deliberately using exactly-representable
  sixteenth-step values. The workspace now enables serde_json's
  `float_roundtrip` feature, making value round-trips exact. Accepted: a
  modest float-parse slowdown (irrelevant at corpus scale), and that
  ensemble parts share tags until a per-part tagging pass exists.

- 2026-06-11 — In the context of the S13 backlog item "pair validator: add
  per-part playability (the S6 filter)" (`core/src/complement.rs` /
  `core/src/fretboard.rs`), facing what *playable* should mean before any
  corpus calibration exists, we decided for **reachability as the verdict,
  fret travel as a fact**: `fretboard::measure_playability` runs the
  existing ADR-0019 fingering DP over a pitch line and summarises its
  optimal path as a `PlayabilityReport` (line notes measured, notes with
  no playable `(string, fret)` under the tuning, and the largest fret
  travel between consecutively positioned notes — never measured across
  an unplayable gap, mirroring the DP's path reset); `validate_pair`
  measures each part's highest-pitch-per-onset line (the closure /
  novelty / gesture convention) under the part's own track `Tuning` with
  the `v1` fingering weights and the standard 24-fret range, and
  `is_clean` now also requires both parts playable — and against folding
  a fret-jump threshold into the verdict (the DP already *minimises*
  travel, so a large jump on the optimal path is real difficulty, but
  where the line sits is tempo- and corpus-dependent — a threshold is
  calibration data for S9/corpus, not code; the fact is carried so S7's
  `playability` / `fret_jump_penalty` cost terms and a future S6
  acceptance filter can consume it), against checking every chord note
  (the DP is monophonic by design — ADR-0019 §7 defers chord voicing; a
  chord participates through its top note, consistent with every other
  line consumer), and against a separate validator entry point (the
  S13 doc asks for the filter *inside* the pair validator; the
  measure itself stays a pub fretboard seam for the S6 filter to reuse).
  Accepted: a part whose difficulty is inter-string stretch rather than
  fret travel reads as playable (string-change cost shapes the DP path
  but is not reported yet), out-of-range notes both fail the verdict and
  hide whatever travel surrounds them, and the `v1` weights remain
  untuned placeholders.

- 2026-06-11 — In the context of the last S13 backlog item "`PartProfile`:
  richer harmonic context (key/scale fit) for pitch material"
  (`core/src/complement.rs`), facing how B should get pitch material when A
  is harmonically sparse (a power-chord riff carries two pitch classes, so
  literal-pitch-class substitution collapses B onto one pitch per band), we
  decided for **the Krumhansl–Schmuckler key estimate as a carried fact,
  and "enrich, don't replace" for material**: `analyze_part` correlates the
  part's duration-weighted pitch-class histogram against the 24 rotated
  Krumhansl–Kessler tonal-hierarchy profiles (prior art: Krumhansl & Kessler
  1982; Krumhansl, *Cognitive Foundations of Musical Pitch*, 1990 — the
  standard key-finding baseline, as implemented in e.g. music21's
  `KrumhanslSchmuckler`; reimplemented natively, no dependency) and carries
  the winner as `PartProfile::harmony` (tonic pitch class, major / natural
  minor, plus `scale_fit` — the duration-weighted fraction of notes on the
  inferred scale, a fact, not a verdict); `scale_intervals_from` unions the
  inferred key's scale into B's substitution material so A's literal pitch
  classes always remain available — and against gating the enrichment on a
  `scale_fit` threshold (what counts as "fitting well enough" is corpus/S9
  calibration data, like the fret-jump and dissonance thresholds before
  it), against replacing A's pitch classes with the inferred scale (B
  should always be able to echo notes A actually plays, and a chromatic A
  would otherwise lose real material to a poorly fitting key), against
  weighted-key variants (Temperley 1999) before any corpus exists to prefer
  one profile set over another, and against estimating per-bar local keys
  (S13 derives one request per part; locality can join when S7 consumes
  the context). Ties resolve deterministically to the earliest key in the
  major-then-minor, C-upward scan; all-zero-duration parts fall back to
  count weighting. Accepted: relative-key confusions inherent to the
  profile method on short diatonic lines, that the natural minor stands in
  for all minor variants, and that the enriched material changes the
  seed-deterministic pitch picks of the substitution modes (S13 is still
  pre-corpus; no golden output depends on them).

- 2026-06-11 — In the context of the S14 deferred refinement "sub-bar
  (beat-level) period detection" (`core/src/structure.rs`), facing how to
  compare beat-sized cells when the bar-level similarity was tuned for
  bar-sized signatures, we decided for **exact-signature autocorrelation at
  beat resolution, verbatim repeats only** (prior art: lag-domain
  autocorrelation of musical surfaces — Brown 1993, *Determination of meter
  of musical scores by autocorrelation*, JASA; self-similarity matrices,
  Foote 1999 — the idea reused natively, no dependency): per-beat
  `(onset-within-cell, pitch)` signatures across a uniform timeline (one
  shared time signature and bar span, the bar dividing evenly into
  `numerator` beats), Jaccard-compared at sub-bar lags `1..numerator` beats,
  gated by the existing `PERIOD_THRESHOLD` with ties to the shortest lag,
  reported as `StructureMetrics::detected_subbar_period_ticks` and persisted
  under corpus schema v5 (optional, default `None`, key skipped when absent
  — the v2/v3/v4 compatibility pattern) — and against reusing
  `bar_similarity` at cell granularity (with at most one onset per cell the
  rhythm floor makes *any* constant-rhythm material clear the threshold at
  a 1-beat lag, and the transposition credit makes almost any two
  single-note cells "transpositions": both components are degenerate at
  this scale and would report a vacuous 1-beat period for nearly
  everything), against a separate sub-bar threshold (a second calibration
  knob with no corpus to calibrate it), and against folding the result into
  `detected_pattern_period_bars`/`_ticks` (their bar-level semantics are
  documented and consumed by `structure_axes`; the refinement is a new
  fact, not a redefinition). Accepted: rhythm-only sub-bar tiles (same
  rhythm, changing pitches) read as *no* sub-bar period under the verbatim
  rule, mixed-meter timelines and non-dividing bars abstain entirely, and
  `StructureControl` cannot yet *request* a sub-bar period (the control-side
  increment stays deferred).

- 2026-06-11 — In the context of Codex P2 on PR #38 (the sub-bar period
  pass, `core/src/structure.rs`), facing sparse riffs where empty-empty
  beat pairs (`set_jaccard(∅, ∅) = 1.0`) alone cleared `PERIOD_THRESHOLD`
  at lag 1 and persisted a false one-beat period into v5 corpus metadata,
  we decided for **excluding empty-empty pairs from the sub-bar lag mean**
  (silence may sit inside a tile but never establishes one; a lag with no
  informative pairs is skipped) — and against gating on a minimum
  non-empty-pair fraction (another calibration knob), and against changing
  the bar-level pass (a rest bar inside a repeated phrase is meaningful at
  bar granularity and that behaviour is documented and tested). Accepted:
  a sub-bar period can now be carried by very few sounded cells on mostly
  silent material — the verbatim rule still requires them to actually
  repeat.

- 2026-06-11 — In the context of the S14 deferred refinement "the full
  per-axis `ComplexityProfile`" (`core/src/structure.rs`), facing what each
  axis should measure before any corpus exists to calibrate against, we
  decided for **untuned v1 facts in `[0, 1]`, built on existing seams**:
  rhythmic and pitch complexity as normalised variety
  (`(distinct − 1) / (count − 1)` — 0 for one repeated value, 1 for
  all-distinct) over inter-onset intervals and over absolute melodic
  intervals along the highest-pitch-per-onset line (the shared line
  convention), technical as the share of notes carrying a per-note mark or
  sitting inside a technique span (both ADR-0018 surfaces), harmonic as
  `1 − scale_fit` of the S13 Krumhansl–Schmuckler key estimate (the
  estimator becomes a `pub(crate)` seam over `(pitch, duration)` pairs so
  complement and structure share one implementation), playability as
  `max_fret_jump / 12` on the ADR-0019 optimal fingering path capped at an
  octave with unreachable notes maxing the axis, and structural as the
  distinct-bar-signature ratio (`bar_signatures` extracted as a shared
  helper; the same fact as `StructureMetrics::structural_complexity`) — and
  against persisting the profile into `ChunkMeta` in the same increment
  (measure before target, ADR-0015: the schema bump joins once the vector
  has consumers), against weighting or aggregating the axes (weights are S9
  data, ADR-0017), and against a syncopation-based rhythmic axis (off-grid
  share needs a grid-resolution choice — a calibration knob; variety needs
  none). Accepted: the axes are coarse (a two-value rhythm scores the same
  variety wherever it sits), playability reads fret travel only (the same
  ADR-0019 limitation the pair validator accepted), and harmonic complexity
  inherits the relative-key confusions of the profile method.

- 2026-06-11 — In the context of giving the S14 `ComplexityProfile` its
  first consumer (`preview/src/analysis.rs` / `tui.rs`), facing whether the
  vector's next step is corpus persistence (schema v6) or a display
  surface, we decided for **the preview inspector first**: `analyze`
  measures the focus track's profile next to the structure metrics, and
  the TUI dock renders it as a compact three-row block of abbreviated axis
  pairs (`rhy`/`pit`, `tec`/`har`, `ply`/`str`) — and against bumping the
  corpus schema in the same increment (the earlier decision stands:
  persistence joins once consumers exist and prove the shape), and against
  one labelled meter row per axis (twelve rows would push the transport
  block out of a 20-row terminal, and the golden frames pin exactly that
  regression). The preview golden tests now honour `GRIFF_BLESS=1` like
  the core characterization snapshots, replacing `include_str!`, so
  intended UI changes re-bless uniformly. Accepted: abbreviated axis
  labels trade self-evidence for fit (the doc comment spells them out),
  and the demo frame shows a hand-filled profile rather than a measured
  one (the demo `Analysis` is a literal, not an `analyze` result).

- 2026-06-11 — In the context of Codex P2 on PR #38, round two (the
  sub-bar period pass, `core/src/structure.rs`), facing `A A B B` reading
  as a one-beat period (two of three lag-1 pairs match, mean 2/3 clears
  `PERIOD_THRESHOLD`) although the documented contract says *verbatim
  tiling*, we decided for **replacing the lag mean with a tiling test**:
  a lag qualifies only when every aligned cell pair matches exactly, the
  shortest qualifying lag wins, and `PERIOD_THRESHOLD` drops out of the
  sub-bar pass entirely (it stays the bar-level mechanism, where graded
  similarity is the point) — this supersedes the same-day "empty-empty
  pairs sit out of the mean" rule, which the all-pairs test subsumes:
  empty cells match empty cells inside a tile, but any mismatch against a
  sounded cell vetoes the lag, so silence still cannot establish a
  period — and against keeping the mean with a higher threshold (any
  threshold below 1.0 admits some non-tiling mix; exactly 1.0 *is* the
  all-pairs test, stated less directly), and against a verbatim-majority
  rule (calibration with no corpus to calibrate on). Accepted: the pass
  is all-or-nothing — a single varied cell hides a sub-bar tile the ear
  would still group (the bar-level repeatability continues to carry
  graded repetition), and the result is no longer accompanied by a
  strength value (a verbatim tile's strength is definitionally 1).

- 2026-06-11 — In the context of Codex P2 on PR #38, round three (the
  sub-bar period pass, `core/src/structure.rs`), facing `A B C A` in one
  4/4 bar reading as a 3-beat period because the lag-3 all-pairs test had
  exactly one pair to check (a tile "observed" once), we decided for
  **capping sub-bar lags at half the cell count** — the same evidence rule
  the bar-level pass has always applied via `max_lag = n / 2`: a period is
  a recurrence claim, so the tile must fit the span at least twice — and
  against requiring the cell count to divide by the lag with modulo-class
  comparison (the comparison half is equivalent to the all-pairs test, and
  divisibility would reject genuine tiles truncated by the bar count — the
  tile/vary compiler explicitly produces truncated final copies). Accepted:
  a true sub-bar idea stated exactly once in a short span goes unreported
  (consistent with the bar-level rule), and single-bar scores can only
  ever report periods up to half a bar.

- 2026-06-11 — In the context of Codex P2 on PR #38 (the preview
  inspector, `preview/src/tui.rs`), facing the dock content exceeding a
  20-row terminal once structure metrics *and* the complexity block are
  both measured (the goldens only covered the metrics-less demo, and the
  live transport block fell off the bottom), we decided for **ordering by
  liveness**: transport (play state, position) renders directly under the
  section info, ahead of the static metrics blocks, so bottom clipping
  always eats the metrics tail — and against compacting the structure
  meters to win the rows back (the meters are the structure block's
  readability; squeezing both blocks to fit every height is a layout
  arms race a scrollable dock should end instead), and against a
  height-conditional layout (two arrangements to characterize for one
  dock). Accepted: on short terminals the structure/complexity tail is
  what clips, and a scrollable or collapsible inspector remains the real
  fix (an S8 follow-up).

- 2026-06-11 — In the context of persisting the S14 `ComplexityProfile`
  (`core/src/corpus.rs`, schema v6), facing when the vector's shape is
  settled enough to freeze into records, we decided for **persisting now
  that a consumer exists**: the preview inspector renders the six axes and
  exercised the shape, so `ChunkMeta` gains the optional `complexity`
  field under the established compatibility pattern (default `None`, key
  skipped when absent; pre-v6 records round-trip byte-identically) and
  `griff curate` measures and stores it with structure and gesture — and
  against folding the profile into `StructureMetrics` (it is a per-track
  complexity fact, not a time-organisation fact; the structural axis
  already shares its value with `structural_complexity` by construction),
  and against a similarity-axes join in the same increment (the
  similarity edge deliberately treats "measured" as an all-or-nothing
  vocabulary per policy version; growing it to v3 with six more axes is
  its own red→green step). Accepted: re-curation is needed before v6
  fields appear on existing records (the established healing path), and
  the v1 axis definitions are now load-bearing for stored data — future
  refinements bump the schema again rather than silently re-meaning
  stored values.

- 2026-06-11 — In the context of Codex P2 on PR #38 (the complexity
  profile's technical axis, `core/src/structure.rs`), facing a technique
  span in one voice marking simultaneous plain notes in *other* voices as
  technical (the spans were flattened track-wide before the coverage
  test, inflating `ComplexityProfile.technical` on polyphonic tracks), we
  decided for **per-voice span scoping**: each voice's notes are tested
  against that voice's spans only — a `TechniqueSpan` is recorded inside
  a voice's event group and describes playing technique on that voice's
  line — and against per-event-group scoping (a span's tick range
  legitimately outlives its anchor group — a palm-mute span covers the
  following notes of the same voice — so group scoping would undercount
  what the span explicitly states). Accepted: two voices genuinely played
  with one physical gesture (rare in practice; importers attach the span
  to one voice) count the technique on the anchored voice only.
