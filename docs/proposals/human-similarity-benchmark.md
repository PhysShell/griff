# Human Similarity Benchmark (design)

Working design artifact for the staged
[preference-and-similarity-learning](preference-and-similarity-learning.md)
proposal: the evidence contract, sampling protocol, session procedure, and
metric-to-gate mapping for the four-task human benchmark that proposal names
as *the gate* (its Stage 2). This document designs the benchmark; it does not
build it.

Status: for discussion (v1). Companion to
[`preference-and-similarity-learning.md`](preference-and-similarity-learning.md);
that proposal stays the thesis, this file is the benchmark spec.
Scope: docs-only; binds nothing. The canonical S9 Phase 1 plan — explainable
EMA weight updates, "no gradient descent / RL before S10" — **remains
governing** until the parent proposal is accepted (its Scope note); nothing
here revises a stage doc.

**Acceptance gate (normative for this document).** This document *designs a
benchmark*. It promises no implementation, no API, no data-collection UI, and
no delivery phase. It grants no metric authority over production: a benchmark
result carries no weight in `rerank.rs`, the S9 Phase 1 EMA plan, or any
scoring policy until the parent proposal's per-task gates are actually run and
that proposal is accepted. Every later implementation slice — an evidence
record type, a sampler, a metric, a collection surface — earns its own spec
section and its own red→green cycle through the normal stage path. A section
here is a licence to *specify*, never a decision already taken.

Licence discipline: all prior art below is idea-only reuse. GPL-licensed
sources never contribute code to this MIT workspace (AGENTS.md rule); the
methodology is reimplemented natively over Griff's own vocabulary.

## 0. What the benchmark is, and what it is not

The benchmark answers one question and refuses to answer three others. It
measures **whether a candidate distance agrees with human judgement on a named
task**. It does not measure musical quality (agreement is not quality — §2.8),
it does not license a metric that wins one task to speak on another (§1), and
it does not touch production until the parent proposal's gates run (the
acceptance gate above).

Two facts from the code decide the shape of everything below, and both are
verified against the current tree, not assumed:

- The scarce resource is human listening, not CPU (the reachability-lab
  premise). A protocol that wastes auditions is the expensive failure; a
  protocol that wastes generation trials is cheap. Every design choice here
  spends CPU to save ears.
- Griff already emits honest, typed provenance for a curator's verdicts
  (`ui-core/src/history.rs`, S8 Slice 3): a `Verdict` that is exactly
  `Favorite | Rejected` held as `Option<Verdict>` (undecided is `None`, and
  favorite/rejected are mutually exclusive *by construction*), an append-only
  `SessionHistory`, and a typed `GeneratorProvenance` whose doc-comment law is
  "never invent a value the pass did not produce". The benchmark extends this
  culture; it does not restart it. Note the honest gap: that verdict model has
  **no `Skip` state** — the benchmark must add one (§2.2), because S9 already
  requires that "absence of a like is not silently treated as a dislike".

## 1. Four benchmark tasks (closed set — exactly four)

The parent proposal already declares these as **separate tasks with separate
datasets, metrics, and gates**, not one similarity function. This set is
closed: the benchmark admits exactly these four tasks. A fifth task is a future
decision recorded by amending this set, never a silent addition.

**A metric that passes one task earns no licence on another.** This is not a
caveat; it is the reason the tasks are separate. *Similarity* rewards
closeness; *complementarity* frequently rewards **difference** — the opposite
gradient — so a distance that ranks motif-similarity well can be actively wrong
about which second guitar complements a lead. Cosine proximity is not human
perception until proven so, per task.

### Per-task field set (field discipline)

Every task below carries **every** field in this set. Where a field is
consciously undetermined, it reads `TBD at spec` (a binding decision deferred
to the implementation slice, with the decision procedure named) or `N/A` (the
field does not apply) — never a silent omission.

- **Question** — the exact wording shown to the listener, over an anchor `A`
  and two candidates `B` / `C`.
- **Anchor source** — where `A` comes from.
- **Candidate source** — where `B` / `C` come from.
- **Gate comparison** — the candidate metric versus the handcrafted baseline
  (§2.6), and what is being compared.
- **Pass criterion** — when the candidate metric passes this task's gate.
- **Gate metric** — which metric of §2.5 reads the gate (named, not implicit).
- **Confounds** — the confounds specific to this task, and how sampling (§2.3)
  controls them.
- **Baseline expressiveness** — an honest statement of whether the handcrafted
  baseline can express this task at all.

### 1.1 similarity — *which is more similar to A?*

- **Question**: "You heard riff **A**. Which of **B** and **C** is more
  *similar* to A?"
- **Anchor source**: a corpus chunk carrying measured structure, gesture, and
  complexity (schema ≥ v6 — the fields `similarity_axes` requires).
- **Candidate source**: two other corpus chunks (or generator variants of the
  anchor), one drawn *near* and one *far* under a coarse pre-stratification so
  the triplet is not a coin-flip; the near/far draw is a sampling device, never
  revealed to the listener and never treated as the ground-truth label.
- **Gate comparison**: for each triplet, the baseline predicts the more-similar
  candidate as the one with the higher similarity aggregate to `A` under
  `similarity_weights_v3` (`core/src/similarity.rs`); the candidate metric
  predicts likewise. Each is scored against the human triplet choice.
- **Pass criterion**: the candidate metric's triplet-agreement lower bound
  (§2.5 bootstrap CI, not the point estimate) is **≥** the handcrafted
  baseline's on the same triplets. Matching, not only beating, passes — per the
  parent proposal's gate rule.
- **Gate metric**: triplet agreement (primary); top-k retrieval, Kendall /
  Spearman rank correlation, and NDCG where a full candidate ranking against
  `A` exists (secondary, §2.5).
- **Confounds**: chunk length, tempo, register, and same-song familiarity.
  Sampling controls length/tempo/register by matching them across `B`/`C` when
  they are not the axis under test (§2.3 confound control), and forbids
  same-song `A`/`B`/`C` except as a labelled special case (§2.3 source
  identity).
- **Baseline expressiveness**: **expressible, with a stated ceiling.**
  `similarity.rs` reads only persisted `ChunkMeta` metadata (structure, tags,
  gesture, complexity) and **no note content** by design — so it is a genuine
  baseline only for anchors/candidates that are *measured chunks*. A benchmark
  item over raw note content that has no measured metadata cannot use this
  baseline; such items are out of the similarity task's scope until a
  note-content distance is specified (`novelty.rs` is note-level but measures
  quotation, not similarity — §2.6).

### 1.2 variation — *which is a better variation of A?*

- **Question**: "You heard riff **A**. Which of **B** and **C** is a better
  *variation* of A — recognisably the same idea, but genuinely varied?"
- **Anchor source**: a corpus chunk or a generator source.
- **Candidate source**: two deterministic generator variants of `A` (the
  reachability-lab / candidate-terrain variation axes: rhythmic displacement,
  contour, register, technique), differing in how far they depart from `A`.
- **Gate comparison**: "good variation" is explicitly **two-sided** — a good
  variation is neither a copy nor a stranger. The baseline is therefore a
  *declared composite* of a closeness term (similarity aggregate, §1.1) and a
  difference term (`novelty.rs` `quote_novelty` / `ngram_novelty` — the share
  of `B` not quoting `A`), scoring highest in a middle band. The candidate
  metric is compared against the same human choice.
- **Pass criterion**: candidate triplet-agreement CI lower bound ≥ baseline, as
  in §1.1.
- **Gate metric**: triplet agreement (primary); pairwise agreement on derived
  labels (secondary).
- **Confounds**: length/tempo/register (matched across `B`/`C` when not under
  test); and the **anchoring confound** — a variant that merely reorders `A`
  reads as "same idea" but is not a variation. Sampling excludes trivial
  identity/transposition-only variants from the candidate pool (they belong to
  copy-detection, §1.4).
- **Baseline expressiveness**: **partially expressible.** No shipped metric
  scores "good variation"; the composite above is a benchmark construct, and
  the exact shape of its middle band (the closeness/difference trade-off and
  its breakpoints) is `TBD at spec`, to be fixed by the implementation slice
  against a held-out calibration set — not guessed here.

### 1.3 complementarity — *which better complements A?*

- **Question**: "**A** is one guitar part. Which of **B** and **C** better
  *complements* A when played together — supports and answers it, rather than
  copying or clashing with it?"
- **Anchor source**: a lead guitar part (corpus track or generator output).
- **Candidate source**: two candidate second-guitar parts against the same
  lead. Same-source (same-song) A/B pairs are a labelled special case (§2.3),
  never a silent default.
- **Gate comparison**: this task rewards **difference that fits**, not
  closeness — a complement that doubles the lead is worse, not better. See the
  expressiveness note: the handcrafted "baseline" here is a legality filter,
  not a preference distance, so the honest comparison is *any candidate metric
  versus a near-absent baseline*.
- **Pass criterion**: `TBD at spec`. Because no handcrafted preference baseline
  exists (below), the pass criterion cannot be "beat the baseline distance" as
  stated for §1.1–§1.2. The decision procedure: the implementation slice
  defines either (a) a random / legality-only baseline as the floor a learned
  complementarity score must clear, or (b) a newly specified handcrafted
  complementarity distance that itself earns a spec section — chosen against
  evidence, not now.
- **Gate metric**: triplet agreement (primary); cross-user generalization
  (§2.5) weighted heavily, because complementarity judgements are expected to
  vary most between listeners.
- **Confounds**: register overlap and rhythmic density between `A` and each
  candidate (a complement that simply sits in a free register can win for the
  wrong reason); loudness/timbre are `N/A` (symbolic, no synthesis — §2.8).
  Sampling matches candidate register-occupancy and density bands across
  `B`/`C` when they are not the axis under test.
- **Baseline expressiveness**: **not expressible by any current metric — stated
  honestly rather than stretched.** `core/src/complement.rs` `validate_pair`
  produces a *legality* verdict (`is_clean`: coincident-dissonance classes
  `{1, 6, 11}` on shared onsets, register-mud `band_overlap > 0.5`, per-part
  playability), it returns a boolean cleanliness fact rather than a graded
  preference, **and it is a validator, not a wired gate** (the arranger returns
  candidates without calling it — verified in the constraint-inventory).
  Stretching a clean/not-clean legality check into a complementarity *distance*
  would be exactly the dishonesty the house rules forbid. Complementarity
  therefore begins with essentially no handcrafted baseline, and the benchmark
  says so.

### 1.4 copy-detection — *which is too close / too far?*

- **Question**: "**A** is an existing riff. One of **B** and **C** is *too
  close* to A to count as original; the other is *too far* to be a variation of
  it. Which is which?" (A two-alternative forced choice over a copy candidate
  and a remote candidate.)
- **Anchor source**: a corpus chunk treated as reference material.
- **Candidate source**: one near-quotation of `A` (verbatim or
  transposed/resolution-shifted) and one musically remote fragment.
- **Gate comparison**: the baseline is `novelty.rs` directly — `quote_novelty`
  and `ngram_novelty` over transition sequences, plus the longest common
  contiguous transition run; the copy candidate is predicted as the one with
  the lower novelty (longer quote). The candidate metric is compared against
  the human "which is the copy" choice.
- **Pass criterion**: candidate agreement CI lower bound ≥ baseline agreement,
  on the same items. Copy-detection is a **classification/threshold** task, not
  a ranking one — so its gate reads agreement, not NDCG or rank correlation
  (those are `N/A` here, §2.5).
- **Gate metric**: triplet agreement read as classification agreement
  (primary); per-user consistency (§2.5) as a data-quality precondition.
- **Confounds**: transposition and tick-resolution — a "copy" a human hears is
  often transposed or re-quantised. Sampling deliberately *includes* transposed
  and resolution-shifted copies (the exact leaks `novelty.rs` is built to catch
  through interval/normalised-IOI transitions), so the task tests robustness,
  not literal byte-matching.
- **Baseline expressiveness**: **expressible on the "too close" side, weaker on
  the "too far" side.** `novelty.rs` measures distance-from-reference as
  quotation share (its `PHRASE_DUPLICATE_SHARE = 0.8` default is an existing
  "too close" threshold), so the copy side has a real baseline. "Too far" has
  no dedicated metric — remoteness is only the *absence* of quotation, not a
  positive measure of musical distance — so the remote-side baseline is `TBD at
  spec`.

## 2. The specification

### 2.1 Overview of what each subsection fixes

§2.2 fixes the evidence record (a Stage 0 red test could be written from it
alone). §2.3 fixes how items are sampled and how leakage and confounds are
controlled. §2.4 fixes the session shape and the evidence floor. §2.5 fixes
which metric reads which task's gate. §2.6 fixes the baselines and states,
per task, exactly what "baseline distance" means. §2.7 is the one paragraph on
the collection surface. §2.8 is the non-goals. §2.9 is the prior art.

### 2.2 Evidence schema

One judgement is one JSONL record. The archive carries a schema identity
string and an integer version, mirroring the `griff.constraint-lab-run`
convention (`lab/src/manifest.rs`): schema identity
`griff.similarity-benchmark-judgement`, version `1`. A record is append-only
and self-describing; a reader that does not recognise the version refuses
rather than guessing.

The record is complete enough that a Stage 0 implementation could be red-tested
from this document alone. Every field is listed; closed vocabularies are
enumerated exactly.

**Identity and task.**

- `schema` (`"griff.similarity-benchmark-judgement"`) and `version` (`1`) —
  the archive identity.
- `task` — closed enum, exactly the four of §1:
  `{ similarity, variation, complementarity, copy_detection }`.
- `question_variant` — an identifier for the exact wording shown (§1 gives one
  wording per task; variants for wording-robustness checks are enumerated in
  the spec slice, not free text).
- `session_id` — the session this judgement belongs to.
- `user_id` — the listener. Distinct from `session_id`: one user runs many
  sessions, and cross-user generalization (§2.5) needs the split. Honest note:
  the S8 playground persists neither a user nor a session id today (it keys on
  monotonic `HistoryId` / `GenerationRunId`); both identities are additions the
  benchmark introduces, and how users are enrolled/pseudonymised is `TBD at
  spec`.

**Item identities (recipe / content / lineage split).** Every stimulus —
anchor and each candidate — is identified by the **three distinct identities**
of the transactional-editing proposal, never collapsed into one:

- `recipe` (`GenerationRecipeId`) — what ran and in which environment
  (source hash, operator/program hash, seed bundle, policy version, generation
  context, contract/schema versions). The reproduction key.
- `content` (`CandidateContentId`) — a hash of the canonical score / selected
  extent. The dedup and "is this the same music?" key; favorites key on this.
- `lineage` (`LineageId`) — which source and transform chain produced it. The
  provenance/history key.

The record carries this triple for `anchor`, and for each member of the
exposure set. Two different recipes can produce byte-identical content; content
identity is what decides whether two stimuli are the same music.

**Exposure and audition.**

- `exposure` — the full ordered set of candidates presented, as their content
  ids, in `display_order` (the order shown on screen). Both are stored: a
  choice is uninterpretable without the display order that framed it (order
  bias is a confound, §2.4).
- `auditioned` — for each exposed candidate, whether it was actually played,
  the `listen_ms` (audition duration in milliseconds), and a `completed` flag
  (did it play to the end). A choice among candidates one of which was never
  heard is a different datum from an informed choice, and downstream analysis
  must be able to drop it.

**Responses (closed vocabularies; unary and choice kept distinct).**

- `choice` — the triplet/forced-choice answer, closed enum
  `{ candidate_b, candidate_c, skip }`. For §1.4 the wording maps "which is the
  copy" onto the same `candidate_b | candidate_c` slots; the semantics live in
  `question_variant`, not in a new response state.
- `skip` is its own state inside `choice`, **distinct from a reject.** A Skip
  means "no judgement offered" (the S9 rule: absence of a like is not a
  dislike); it is never derived into a pairwise label. This is the state the
  current `Verdict` model lacks (§0).
- `unary` — optional, per audited candidate: `{ favorite, reject }` or absent
  (undecided), modelling the exact `Option<Verdict>` shape of
  `ui-core/src/history.rs` (favorite and reject mutually exclusive; absent is
  undecided). Unary signals are **kept separate** from `choice` in the record.
- **Pairwise labels are derived, never stored raw and never conflated.** A
  record stores the triplet `choice` and the `unary` signals as *observations*;
  an "A-over-B" pairwise label is computed downstream from a `choice` under a
  documented derivation, and a unary favorite/reject is **not** a pairwise
  label (a favorite is not a win against every unexposed neighbour — the parent
  proposal's Stage 0 rule). The derivation policy is versioned separately from
  this record.

**Ordering / timing.** A monotonic `sequence` index within the session fixes
record order without a wall-clock dependency in the identity; wall-clock
`listen_ms` is a measured human quantity, not part of any deterministic key.

### 2.3 Sampling protocol

- **Stratification over corpus axes.** Items are stratified over the schema-v6
  per-axis `ComplexityProfile` (rhythmic, pitch, technical, harmonic,
  playability — `core/src/structure.rs`) and over the structure/gesture facts
  and `SwancoreTag` categories (style / harmony / technique / rhythm /
  structure — `core/src/corpus.rs`), so no task is silently dominated by one
  region of the corpus (e.g. only clean single-note riffs).
- **Confound control (timbremetrics methodology).** When a task does not test a
  dimension, that dimension is controlled by matched sampling: for a similarity
  triplet not testing length, `B` and `C` are drawn in the same length band as
  each other; likewise tempo and register. The controlled dimensions per task
  are named in §1. Symbolic-only dimensions that have no analogue (loudness,
  timbre) are `N/A`, not silently ignored.
- **Source-identity discipline (the reachability-lab holdout law).** Any
  evaluation split — train/validation/test for a learned metric, or held-out
  users/items for generalization — is by **source identity (song), never by
  chunk**. A split that puts one bar of a song in train and the next bar in
  test measures recognition of the neighbouring bar, not musical understanding.
  Source-identity exclusion is enforced on the item set *before* triplets are
  assembled, exactly as the reachability lab enforces holdout before material
  construction.
- **Same-song candidate pairs are a labelled special case.** A triplet whose
  `A` and a candidate share a song (unavoidable for §1.3 complementarity, where
  the two guitars come from one arrangement, and deliberate for some §1.4 copy
  items) is **tagged as such in the item manifest**, never presented as if it
  were a cross-song pair. Analysis can then include or exclude same-song items
  explicitly, and a same-song win is never quietly counted as cross-song
  generalization.

### 2.4 Session procedure

- **Triplets per session (fatigue bound).** A session presents a bounded number
  of triplets so judgements late in a session are not fatigue artefacts.
  Proposed bound: **≤ 30 triplets per session**, from the listening-test
  hygiene literature (attention and consistency degrade over long forced-choice
  runs; ~20–40 comparisons is the usual working ceiling). The binding number is
  `TBD at spec` — decided by measuring per-user consistency (§2.5) as a
  function of within-session position on pilot data, and setting the bound
  where consistency starts to fall.
- **Order randomization.** `display_order` within each triplet, and triplet
  order within a session, are randomized per session (and the realized order is
  recorded, §2.2), so left/right and early/late position cannot masquerade as
  preference.
- **Repeated probes (per-user consistency).** A fraction of triplets are
  repeated within or across a user's sessions (identical content ids, re-drawn
  display order) to measure per-user self-agreement. A user whose repeat
  self-agreement is at chance contributes noise, not signal, and their
  judgements are down-weighted or excluded under a documented rule.
- **Minimum evidence threshold.** No task gate (§2.5) may be *read* until a
  floor of evidence exists, so a lucky handful of triplets cannot "pass" a
  task. Proposed floors, per task: **≥ 50 triplets per user per task** (enough
  for a per-user bootstrap CI to be narrower than the baseline gap the gate
  tests), and **≥ 5 distinct users** for any cross-user claim. Both numbers are
  `TBD at spec`: the decision procedure is to fix them from the target bootstrap
  CI width (§2.5) — the floor is whatever makes the CI lower bound a meaningful
  test rather than noise — not to assert them by taste here.

### 2.5 Metrics, and which metric gates which task

The metric family is the one named in the parent proposal. What this section
adds is the **explicit mapping** — no task's gate is left to be inferred.

| Metric | Role | Gates which task(s) |
| --- | --- | --- |
| Triplet agreement | primary forced-choice agreement | **all four** (the primary gate metric everywhere) |
| Pairwise agreement | agreement on *derived* pairwise labels | similarity, variation (secondary) |
| Top-k retrieval | ranked-pool recovery against an anchor | similarity only |
| Rank correlation (Kendall / Spearman) | ordering agreement over a candidate pool | similarity only (`N/A` for copy-detection) |
| NDCG | graded ranking quality | similarity only (`N/A` for copy-detection) |
| Per-user consistency | repeated-probe self-agreement | **all four**, as a data-quality *precondition* read before the gate |
| Cross-user generalization | held-out-user agreement | **all four**; weighted heaviest for complementarity (§1.3) |
| Bootstrap confidence intervals | uncertainty envelope on every agreement number | **all four**; the gate compares CI **lower bounds**, never point estimates |

Two mapping rules make the table binding rather than decorative:

- **The gate is the CI lower bound.** A task passes only when the candidate
  metric's bootstrap-CI lower bound on its gate metric is ≥ the baseline's on
  the same items (§1 pass criteria). A point-estimate win inside overlapping
  CIs does not pass.
- **Ranking metrics gate only similarity.** Top-k, Kendall/Spearman, and NDCG
  require a graded ranking of a candidate pool against `A`; that structure
  exists for similarity, is secondary/partial for variation, and does **not**
  exist for the forced binary of copy-detection — so they are `N/A` there, and
  reporting NDCG on a copy-detection item is an error, not a bonus number.

### 2.6 Baselines and compared systems

The handcrafted baselines are **named code**, and "baseline distance" is
defined per task rather than assumed uniform. Learned embeddings and learned
distances enter later and, per the parent proposal's gate rule, must **beat or
match** the baseline *on that task* before earning any production use — a win
on one task is no licence on another (§1).

- **similarity** — baseline distance = the complement of the similarity
  aggregate under `similarity_weights_v3` (`core/src/similarity.rs`): the
  uniform-policy `WeightPolicy` over the named `SIMILARITY_AXIS_LABELS`
  (period / repeatability / loopability / structural-complexity / tags, the
  gesture axes, and the schema-v6 complexity axes). It reads persisted
  `ChunkMeta` metadata and **no note content**. Real baseline for measured
  chunks; inapplicable to unmeasured raw content (§1.1 ceiling).
- **variation** — no single baseline metric exists; the baseline is a declared
  composite of the similarity aggregate (closeness) and `novelty.rs`
  `quote_novelty` / `ngram_novelty` (difference), peaking in a middle band
  whose breakpoints are `TBD at spec` (§1.2).
- **complementarity** — **honestly, no baseline distance.**
  `complement.rs::validate_pair` is a legality validator (and not a wired
  gate), producing `is_clean` rather than a graded preference. It filters the
  illegal; it does not rank the complementary. The benchmark states this rather
  than stretching a boolean into a distance; the floor a learned complementarity
  score must clear is a random/legality-only baseline or a newly specified
  handcrafted distance (§1.3), decided at spec.
- **copy-detection** — baseline distance = `novelty.rs` quotation facts
  (`quote_novelty`, `ngram_novelty`, longest common contiguous transition run),
  which are transposition- and resolution-robust by construction. Strong
  baseline on the "too close" side; the "too far" side has no positive-distance
  metric and is `TBD at spec` (§1.4).

Compared systems, in the order the parent proposal admits them: handcrafted
symbolic distances (above); corpus features; learned embeddings (Stage 1,
retrieval-only until they pass a gate); and combinations. Nothing learned
enters production scoring for a task until it clears that task's gate — the
line this whole document exists to hold.

### 2.7 Collection surface

The intended surface is a new **A/B/C mode of the S8 playground**, which
already persists favorite/reject/history with typed provenance
(`ui-core/src/history.rs`; `cockpit/src/generation.rs`) — the benchmark reuses
that append-only, backend-neutral evidence spine and adds the triplet
presentation, the `skip`/`choice` responses, session/user identity, and the
audition timing of §2.2. **UI implementation is out of scope of this proposal**
— no mockups, no widget contracts, no `ui-core` changes are specified or
implied here; that surface earns its own spec section and red→green slice
through the S8 path when the parent proposal is accepted.

### 2.8 Non-goals

- No implementation, no API, no data-collection UI, and no delivery phase (the
  acceptance gate).
- No embeddings, no learned distance, and no training here — the benchmark
  *evaluates* them; Stage 1 embeddings stay retrieval-only until they pass a
  gate.
- **No change to `rerank.rs`, to any frozen strategy, or to the S9 Phase 1 EMA
  plan, which remains the governing plan.** Acceptance of the *parent* proposal
  is what revises S9; this companion revises nothing.
- Benchmark results carry **no production authority** until the parent
  proposal's per-task gates are run and it is accepted — no metric, weight, or
  threshold flows into scoring on the strength of a number in this document.
- **No audio similarity.** Griff is symbolic (MIDI in → MIDI out, no synthesis);
  timbre and loudness confounds are `N/A`, and no claim here concerns perceived
  sound.
- **No claim that agreement proves musical quality.** A metric that agrees with
  human similarity judgements is a metric that agrees with human similarity
  judgements — nothing more. Quality, taste, and "is this a good riff" are out
  of scope; conflating agreement with quality is the error this non-goal names.

### 2.9 Prior art (idea-only; licences noted)

Surveyed before designing, per the prior-art-first rule; the survey is recorded
canonically in `docs/decisions.log.md` (dated entry), because a proposal is a
transient discussion artifact and the rule requires a durable trace outside it.
Idea reuse only — no code from any source below enters this MIT workspace.

- **timbremetrics / timbre-dissimilarity-metrics** — the human-similarity
  benchmark methodology this document transposes to symbolic riffs: triplet
  forced-choice agreement, top-k agreement, the Mantel test for
  matrix-vs-matrix correlation, and datasets built with **controlled
  confounds** (the discipline behind §2.3's matched sampling). Adopted as
  method; not as code.
- **Pairwise-aggregation literature (Bradley–Terry / TrueSkill-style)** —
  referenced only for how *derived* pairwise labels (§2.2) could later be
  aggregated into a latent preference/skill score. Idea noted; no aggregation
  model is specified or adopted here — the benchmark stores observations and
  derives labels under a versioned policy, and any aggregation is a later
  decision.
- **Listening-test hygiene** — the fatigue bound and repeated-probe consistency
  checks of §2.4 follow standard forced-choice listening-test practice
  (bounded comparison counts, randomized presentation order, catch/repeat
  trials for per-listener reliability). Applied as protocol hygiene, not as any
  specific published apparatus.

The parent proposal's own prior-art section (PCGNN novelty decomposition,
learning-to-rank, MES/Bayesian optimization) governs the stages *around* the
benchmark; this document reuses those surveys by reference rather than
duplicating them.
