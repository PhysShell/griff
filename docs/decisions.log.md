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
