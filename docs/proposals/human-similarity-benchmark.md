# Human Similarity Benchmark (design)

Working design artifact for the staged
[preference-and-similarity-learning](preference-and-similarity-learning.md)
proposal: the evidence contract, sampling protocol, session procedure,
presentation contract, and metric-to-gate mapping for the four-task human
benchmark that proposal names as *the gate* (its Step 2, §4). This document
designs the benchmark; it does not build it.

Status: for discussion (v2.2 — v2.1 revised per the third PR #153 arbiter
review, four surgical corrections: copy-detection candidate-source made
constructible under its boundary strata (a non-copy need not be remote); the
`complementarity_mix_v1` question made order-neutral to match randomized
playback; the shared gate-comparison / pass-criterion and the parent gate rule
taught the baseline-vs-floor distinction (non-inferiority / superiority /
ungateable); and the statistical parameters given their own immutable
`griff.similarity-benchmark-gate-run` v1 identity, separate from the judgement
archive. v2.1 — the second-review consistency pass: parent learner +
copy-detection wording
synchronized to the contextual/single-question forms; audition order
randomized and recorded independently of screen layout; difficulty strata
made task-specific (they were not constructible for copy-detection or
complementarity under one near/far scale); paired tie handling made symmetric
across both predictors; the complementarity floor put behind a superiority
gate; per-stratum evidence floors added; concrete v1 policy ids bound; stale
metadata cleaned. v2 itself introduced the paired-difference cluster-bootstrap
gate, corrected B-over-C semantics, the presentation profile, immutable
item-manifest binding, single-question copy-detection, and the `Stage`→`Step`
relabelling.) Companion to
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
record type, a sampler, a metric, a presentation surface — earns its own spec
section and its own red→green cycle through the normal stage path. A section
here is a licence to *specify*, never a decision already taken.

Licence discipline: all prior art below is idea-only reuse. GPL-licensed
sources never contribute code to this MIT workspace (AGENTS.md rule); the
methodology is reimplemented natively over Griff's own vocabulary.

## 0. What the benchmark is, and what it is not

The benchmark answers one question and refuses to answer three others. It
measures **whether a candidate distance agrees with human judgement on a named
task**. It does not measure musical quality (agreement is not quality — §2.9),
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

**Sequence labels.** The parent proposal's `Step 0…3` (and this document's
references to them) are proposal-local sequence labels, not roadmap stage
numbers (glossary §0). "Step 2" is the parent's §4, not a canonical `SN`
stage. The only roadmap stage in play is S9.

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
- **Gate comparison** — the candidate metric versus the task's **named
  handcrafted baseline or explicitly declared floor** (§2.7), and what is
  compared. The comparison is always the **paired, per-item** difference of §2.6
  (candidate correctness minus baseline/floor correctness on the *same* item),
  never two independently computed scores.
- **Pass criterion** — the **task-appropriate §2.6 gate** on that paired
  difference: non-inferiority against a real baseline, superiority against a
  floor, or ungateable where neither is defensible.
- **Gate metric** — the metric of §2.6 that reads the gate (named, not
  implicit). In v1-of-the-benchmark the only gate metric is triplet agreement;
  ranking metrics are diagnostics (§2.6), not gates.
- **Confounds** — the confounds specific to this task, and how sampling (§2.4)
  and the presentation profile (§2.3) control them.
- **Baseline expressiveness** — an honest statement of whether the handcrafted
  baseline can express this task at all.

### 1.1 similarity — *which is more similar to A?*

- **Question**: "You heard riff **A**. Which of **B** and **C** is more
  *similar* to A?"
- **Anchor source**: a corpus chunk carrying measured structure, gesture, and
  complexity (schema ≥ v6 — the fields `similarity_axes` requires).
- **Candidate source**: two other corpus chunks (or generator variants of the
  anchor), drawn under this task's **distance-margin strata** (§2.4) — from
  `wide_margin` through `tie_margin`, plus a `same_stratum_distinct_motif`
  hard-negative — so the pool is not dominated by trivially separable pairs. The
  stratum draw is fixed *before* collection and is **independent of the
  candidate metric under test** (§2.4), so the benchmark cannot degenerate into
  "does the new metric agree with the sampler".
- **Gate comparison**: per triplet, both the baseline and the candidate metric
  predict the more-similar candidate; each prediction is scored right/wrong
  against the human choice; the gate reads the **paired difference** of those
  correctness indicators (§2.6). The baseline's prediction is the candidate
  with the higher similarity aggregate to `A` under `similarity_weights_v3`
  (`core/src/similarity.rs`).
- **Pass criterion**: the §2.6 non-inferiority gate holds — the cluster-
  bootstrap CI lower bound of the paired difference (candidate − baseline) is
  `≥ −δ`, per stratum **and** in aggregate.
- **Gate metric**: triplet agreement (the only gate metric). Top-k retrieval,
  Kendall / Spearman, and NDCG are **exploratory diagnostics, not gates**
  (§2.6) — they need a ranked candidate pool the triplet protocol does not
  produce.
- **Confounds**: chunk length, tempo, register, same-song familiarity, and the
  **presentation** confounds of §2.3 (patch, velocity, gain, loop count).
  Sampling matches length/tempo/register across `B`/`C` when not under test
  (§2.4); the presentation profile fixes the audible-rendering confounds;
  same-song `A`/`B`/`C` is forbidden except as a labelled special case (§2.4).
- **Baseline expressiveness**: **expressible, with a stated ceiling.**
  `similarity.rs` reads only persisted `ChunkMeta` metadata (structure, tags,
  gesture, complexity) and **no note content** by design — so it is a genuine
  baseline only for anchors/candidates that are *measured chunks*. Items over
  raw note content with no measured metadata are out of this task's scope until
  a note-content distance is specified (`novelty.rs` is note-level but measures
  quotation, not similarity — §2.7).

### 1.2 variation — *which is a better variation of A?*

- **Question**: "You heard riff **A**. Which of **B** and **C** is a better
  *variation* of A — recognisably the same idea, but genuinely varied?"
- **Anchor source**: a corpus chunk or a generator source.
- **Candidate source**: two deterministic generator variants of `A` (the
  reachability-lab / candidate-terrain variation axes: rhythmic displacement,
  contour, register, technique), differing in how far they depart from `A`,
  drawn under this task's middle-band strata (§2.4: `clearly_better` /
  `close_call` / `both_off_band`).
- **Gate comparison**: "good variation" is explicitly **two-sided** — a good
  variation is neither a copy nor a stranger. The baseline is a *declared
  composite* of a closeness term (similarity aggregate, §1.1) and a difference
  term (`novelty.rs` `quote_novelty` / `ngram_novelty` — the share of a
  candidate not quoting `A`), scoring highest in a middle band; its prediction
  is compared to the human choice under the same paired-difference gate.
- **Pass criterion**: the §2.6 non-inferiority gate, per stratum and aggregate.
- **Gate metric**: triplet agreement. Pairwise agreement on *derived* labels
  (§2.2) is a secondary diagnostic.
- **Confounds**: length/tempo/register (matched when not under test);
  presentation confounds (§2.3); and the **anchoring confound** — a variant
  that merely reorders `A` reads as "same idea" but is not a variation.
  Sampling excludes trivial identity/transposition-only variants (they belong
  to copy-detection, §1.4).
- **Baseline expressiveness**: **partially expressible.** No shipped metric
  scores "good variation"; the composite above is a benchmark construct, and
  the shape of its middle band (the closeness/difference trade-off and its
  breakpoints) is `TBD at spec`, fixed by the implementation slice against a
  held-out calibration set — not guessed here.

### 1.3 complementarity — *which better complements A?*

- **Question** (`complementarity_mix_v1`, order-neutral so the closed wording
  cannot contradict the randomized playback of §2.3): "**A** is one guitar
  part. You will hear **A paired with B** and **A paired with C**, in a
  randomized order. Which pairing works better — the second guitar supporting
  and answering A, rather than copying or clashing with it?" The listener
  judges the **mixes `A+B` and `A+C`**, never `B` and `C` in isolation; the UI
  labels the pairings consistently after playback without promising which was
  heard first (§2.3 presentation procedure).
- **Anchor source**: a lead guitar part (corpus track or generator output).
- **Candidate source**: two candidate second-guitar parts against the same
  lead, drawn under this task's controlled-feature margin strata (§2.4:
  `wide_margin` / `narrow_margin` / `tie_margin` over a pre-registered
  register-occupancy / density margin independent of every evaluated candidate
  metric — explicitly not a near/far ordering). Same-source (same-song)
  `A`/candidate pairs are a labelled special case (§2.4), never a silent
  default.
- **Gate comparison**: this task rewards **difference that fits**, not
  closeness — a complement that doubles the lead is worse. Because no
  handcrafted preference distance exists (below), the paired-difference gate
  runs the candidate metric against the **declared floor baseline** of §2.7
  (a random / legality-only predictor), not against a similarity distance.
- **Pass criterion**: a **superiority** gate, not the non-inferiority form used
  for §1.1–§1.2 — because "no worse than a coin toss (or a legality filter)" is
  not evidence a learned complement ranker is worth production authority.
  Against a random / legality-only floor the gate is `lower_CI(mean Δ) ≥ ε`
  with a **pre-registered `ε > 0`** (§2.6), i.e. the candidate must *beat* the
  floor by a margin, not merely match it. `TBD at spec` in one respect only —
  *which* floor (random, legality-only via `validate_pair`, or a newly
  specified handcrafted complementarity distance that earns its own spec
  section and then uses the ordinary non-inferiority gate); the decision
  procedure is to pick it from pilot evidence. An honest alternative the spec
  may take instead: declare complementarity **ungateable** until a meaningful
  handcrafted baseline exists — "match random and enter production" is not on
  the table.
- **Gate metric**: triplet agreement, with **cross-user generalization** (§2.6)
  weighted heavily, because complementarity judgements are expected to vary
  most between listeners.
- **Confounds**: register overlap and rhythmic density between `A` and each
  candidate (a complement that simply sits in a free register can win for the
  wrong reason); and — critically for a *mix* judgement — the
  **lead/candidate mix balance** (relative gain, pan), fixed by the
  presentation profile's `lead_candidate_mix_policy` (§2.3). Sampling matches
  candidate register-occupancy and density bands across `B`/`C` when not under
  test.
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
  says so — the gate measures a learned score against a floor, not against a
  distance pretending to rank complements.

### 1.4 copy-detection — *which candidate is too close to A to be original?*

- **Question**: "**A** is an existing riff. Which of **B** and **C** is *too
  close* to A to count as original?" A single two-alternative forced choice,
  asking **one** thing.
- **Anchor source**: a corpus chunk treated as reference material.
- **Candidate source**: two candidates sampled under the copy-detection strata
  of §2.4. The `obvious_copy_vs_remote` stratum uses a near-quotation of `A`
  (verbatim or transposed/resolution-shifted) versus a **remote control**; the
  boundary strata (`threshold_adjacent_copy_vs_noncopy`, `two_threshold_adjacent`)
  use candidates on or near opposite sides of the pre-registered copy threshold
  — **a non-copy candidate need not be remote**. In every stratum the listener
  is asked only "which is the copy": the non-chosen candidate is a control, the
  listener is never asked to rule it "too far", and no "too far" label is
  derived from the record. (The task previously bundled "too close / too far"
  into one click; a 2AFC yields only which candidate looks like the copy, so
  the second, unasked judgement is dropped.)
- **Gate comparison**: the baseline is `novelty.rs` directly — `quote_novelty`
  and `ngram_novelty` over transition sequences, plus the longest common
  contiguous transition run; it predicts the copy as the lower-novelty
  candidate. The candidate metric's prediction is scored against the human
  "which is the copy" choice under the paired-difference gate (§2.6).
- **Pass criterion**: the §2.6 non-inferiority gate. Copy-detection is a
  **classification** task, not a ranking one — its gate reads classification
  agreement, and NDCG / rank correlation are `N/A` here (§2.6).
- **Gate metric**: triplet agreement read as classification agreement; per-user
  consistency (§2.6) as a data-quality precondition.
- **Confounds**: transposition and tick-resolution — a "copy" a human hears is
  often transposed or re-quantised. Sampling deliberately *includes* transposed
  and resolution-shifted copies (the exact leaks `novelty.rs` catches through
  interval / normalised-IOI transitions), so the task tests robustness, not
  literal byte-matching.
- **Baseline expressiveness**: **expressible on the copy side.**
  `novelty.rs` measures distance-from-reference as quotation share (its
  `PHRASE_DUPLICATE_SHARE = 0.8` default is an existing "too close" threshold),
  so the copy prediction has a real baseline. There is no positive "remoteness"
  metric, but the task no longer asks for one — the non-chosen candidate is a
  control, so no `TBD` remoteness baseline is owed.

## 2. The specification

### 2.1 Overview of what each subsection fixes

§2.2 fixes the evidence record and its immutable bindings (a Step-0 red test
could be written from it alone). §2.3 fixes how a stimulus is *presented* to a
listener (the presentation profile and procedure). §2.4 fixes how items are
sampled, stratified by difficulty, and how leakage and confounds are
controlled. §2.5 fixes the session shape and the evidence floor. §2.6 fixes the
statistical gate and which metric reads it. §2.7 fixes the baselines and states,
per task, exactly what "baseline distance" means. §2.8 is the one paragraph on
the collection surface. §2.9 is the non-goals. §2.10 is the prior art.

### 2.2 Evidence schema

One judgement is one JSONL record. The archive carries a schema identity
string and an integer version, mirroring the `griff.constraint-lab-run`
convention (`lab/src/manifest.rs`): schema identity
`griff.similarity-benchmark-judgement`, version `1`. A record is append-only
and self-describing.

**Every referenced policy and manifest is bound by immutable identity, and an
unknown version is a typed refusal, never a guess.** A record does not merely
*name* the item set, sampler, presentation, and derivation it belongs to — it
binds their exact versions, so the same JSONL can never be silently reworked
into a different dataset a year later while the human still "changed nothing".
A reader that does not recognise any bound version refuses. The **gate policy
is deliberately *not* among these bindings** — collection precedes evaluation,
so the statistical parameters live in a separate gate-run manifest (§2.6) that
binds *this* archive's hash, letting the same evidence be re-gated without
rewriting a byte.

The record is complete enough that a Step-0 implementation could be red-tested
from this document alone. Every field is listed; closed vocabularies are
enumerated exactly.

**Archive and binding identity.**

- `schema` (`"griff.similarity-benchmark-judgement"`) and `version` (`1`).
- `item_id` — the item this judgement is about.
- `item_manifest_schema` + `item_manifest_hash` — the immutable item manifest
  (§2.4) the item is drawn from, bound by content hash. Source identities,
  same-source flags, complexity strata, difficulty stratum, and controlled
  dimensions live in that manifest; the record binds them by hash rather than
  re-copying them, so they cannot drift per record.
- `sampling_policy_id` — the versioned sampler that produced the item set.
- `presentation_profile_id` — the versioned presentation profile (§2.3) the
  listener actually heard the item under.
- `derivation_policy_id` — the versioned policy under which pairwise labels are
  derived from this record (below). The accepted v1 policy is
  `contextual_bc_preference_v1` (the corrected B-over-C / C-over-B derivation);
  an unknown id is a typed refusal.
- `dataset_split_id` — the versioned split (train / validation / test, and
  held-out-user membership) this item belongs to (§2.4, §2.6).

**Task and identity.**

- `task` — closed enum, exactly the four of §1:
  `{ similarity, variation, complementarity, copy_detection }`.
- `question_variant` — a value from a **closed, versioned question-variant
  registry** (not a free-form string). The v1 registry is exactly one wording
  per task — `{ similarity_v1, variation_v1, complementarity_mix_v1,
  copy_detection_v1 }` (the §1 wordings); robustness variants are later
  registry entries, each with its own id. An unknown id is a typed refusal
  (below).
- `session_id` — the session this judgement belongs to.
- `user_id` — the listener, as an **irreversible pseudonym** under the privacy
  contract below. Distinct from `session_id`: one user runs many sessions, and
  cross-user generalization (§2.6) needs the split. Honest note: the S8
  playground persists neither a user nor a session id today (it keys on
  monotonic `HistoryId` / `GenerationRunId`); both identities are additions the
  benchmark introduces.

**Item identities (recipe / content / lineage split).** Every stimulus —
anchor and each candidate — is identified by the **three distinct identities**
of the transactional-editing proposal, never collapsed into one:

- `recipe` (`GenerationRecipeId`) — what ran and in which environment. The
  reproduction key.
- `content` (`CandidateContentId`) — a hash of the canonical score / selected
  extent. The dedup and "is this the same music?" key; favorites key on this.
- `lineage` (`LineageId`) — which source and transform chain produced it. The
  provenance/history key.

The record carries this triple for `anchor`, and for each member of the
exposure set. Two different recipes can produce byte-identical content; content
identity is what decides whether two stimuli are the same music.

**Exposure and audition.** Audition evidence covers **the anchor as well as the
candidates** — a judgement from a listener who never heard `A` to the end is a
different datum from an informed one, and must be filterable.

- `exposure` — the full ordered set of candidates presented, as their content
  ids, in `display_order` (the on-screen order). Both stored: a choice is
  uninterpretable without the display order that framed it (order bias — §2.5).
- `audition_order` — the **realized playback order** of the stimuli, recorded
  **independently of `display_order`** (a stimulus can sit second on screen yet
  play last). This is the order randomized in §2.3/§2.5 to break the
  recency confound, and the field the analysis conditions on — screen layout
  and playback order are two different confounds and are stored separately.
- `anchor_audition` — `{ anchor_played, anchor_listen_ms, anchor_completed,
  anchor_replay_count }`.
- `candidate_audition` — per exposed candidate: `{ auditioned, listen_ms,
  completed, replay_count }`. For the mix tasks (§1.3), each *playback identity*
  actually rendered (`A+B`, `A+C`) is recorded, not merely the isolated
  candidate, because the mix is what was judged.

**Responses (closed vocabularies; unary and choice kept distinct).**

- `choice` — the forced-choice answer, closed enum
  `{ candidate_b, candidate_c, skip }`. For §1.4 the wording maps "which is the
  copy" onto the same `candidate_b | candidate_c` slots; the semantics live in
  `question_variant`, not in a new response state.
- `skip` is its own state inside `choice`, **distinct from a reject.** A Skip
  means "no judgement offered" (the S9 rule: absence of a like is not a
  dislike); it is never derived into a pairwise label. This is the state the
  current `Verdict` model lacks (§0).
- `unary` — optional, per audited candidate: `{ favorite, reject }` or absent
  (undecided), modelling the exact `Option<Verdict>` shape of
  `ui-core/src/history.rs`. Unary signals are **kept separate** from `choice`.

**Derived pairwise labels — corrected semantics.** The listener compares `B`
and `C` *relative to `A` under a task*; they never compared `A` with `B`. The
only labels derivable from a `choice` are therefore **B-over-C** or
**C-over-B** *in the context of `A` and the task* — an "A-over-B" label is not
observed and must never be minted:

```text
PairwisePreference {
    context_anchor: A,          // the anchor the comparison was relative to
    preferred:      B,          // the chosen candidate
    dispreferred:   C,          // the other candidate
    task,                       // which of the four (a label from one task is
                                // never a label for another)
    question_variant,
    derivation_policy_id,       // the versioned rule that produced this label
}
```

Labels are **derived, never stored raw, and never conflated with unary
signals**: a record stores the `choice` and `unary` as *observations*; a
`PairwisePreference` is computed downstream under the named
`derivation_policy_id`, and a unary favorite/reject is **not** a pairwise label
(a favorite is not a win against every unexposed neighbour — the parent
proposal's Step-0 rule). A `skip` yields no label. Because
`derivation_policy_id` is bound in the record, two runs of the derivation over
the same JSONL either produce the same labels or declare a different policy.

**Ordering / timing.** A monotonic `sequence` index within the session fixes
record order without a wall-clock dependency in any identity; wall-clock
`listen_ms` values are measured human quantities, not part of any deterministic
key.

**Privacy contract (before any collection).** `user_id` is a real person, so a
Step-0 slice that persists it earns a privacy contract *before* it runs, not
after: an **irreversible pseudonym** (no reconstruction of identity from the
archive), documented **access rules** (who may read raw per-user judgements),
and a **retention** policy (how long raw records live, and what is kept after).
The exact mechanism is `TBD at spec`, but its *existence* is a precondition of
collection, not a later cleanup — "we will sort out identifying people later"
is the first line of an incident report, not a design.

### 2.3 Presentation contract

The object judged is symbolic, but a human hears it through a synth or sampler,
and the audible rendering influences the choice. Timbre and loudness are
therefore **not `N/A`** — they are *presentation confounds*, controlled by a
versioned profile rather than waved away. ("No audio similarity" stays a
non-goal, §2.9: the benchmark makes no claim about perceived *sound* — but it
must still fix *how* the symbolic object is played, or every listener judges a
different rendering.)

**`PresentationProfile` (versioned).** A profile fixes the audible channel;
its id is bound in every record (§2.2):

```text
presentation_profile_id
playback_backend            // e.g. the cockpit's Web Audio / MIDI-out path
instrument_or_patch         // the sound the symbolic notes are rendered with
tempo_policy                // fixed tempo, or the item's own — stated, not implicit
velocity_policy             // how symbolic dynamics map to loudness
gain_and_pan_policy         // per-part gain and pan
loop_count                  // how many times a stimulus repeats before a choice
lead_candidate_mix_policy   // for §1.3: relative gain/pan of lead vs complement
normalization_version       // loudness normalization across stimuli
```

**Procedure.** Every task presents under one profile, with replays permitted
under the *same* profile and counted (§2.2). The anchor is always heard first;
the **two candidate stimuli are presented in a randomized order**, so neither
is systematically the most recent audition before the choice (a recency
confound the fixed `A → B → C` order of v2 left uncontrolled):

- similarity / variation / copy-detection: `A`, then the candidates in a
  randomized order — `A → B → C` or `A → C → B`.
- complementarity: `A` solo, then the two mixes in a randomized order —
  `A → A+B → A+C` or `A → A+C → A+B` — the listener judges the mixes, never
  the candidates alone (§1.3).

The **realized order is recorded in `audition_order`** (§2.2),
**independently of `display_order`**, and the analysis conditions on it. All
realized playback identities (which stimulus, under which profile, for how
long) enter the audition evidence. A judgement whose stimuli were rendered
under an unrecognised profile version is refused, not silently pooled with
another profile's data.

### 2.4 Sampling protocol

- **Difficulty strata (against too-easy and circular triplets) —
  task-specific, not one global scale.** Candidate pairs are drawn into named
  difficulty strata, fixed *before* collection and **independent of the
  candidate metric under evaluation** — so the benchmark cannot reduce to "does
  the new metric agree with the sampler". A single near/far scale is not
  constructible for every task (a copy-detection item is *defined* as
  copy-vs-remote, so it has no "near vs near" form; complementarity has no
  natural near/far ordering at all — closeness to `A` can make a second guitar
  *worse*). Each task therefore declares **its own** strata over a
  controlled-feature margin, never a borrowed near/far label, and each task
  populates **only its own** declared strata:
  - **similarity** — distance-margin strata: `wide_margin` / `mid_margin` /
    `narrow_margin` / `tie_margin` (the controlled similarity-feature gap
    between the two candidates, from large down to ≈ 0), plus a
    `same_stratum_distinct_motif` hard-negative (comparable feature profile,
    different motif identity).
  - **variation** — strata by distance from the middle-band variation optimum
    (§1.2): `clearly_better` / `close_call` / `both_off_band` (one too close +
    one too far vs two similarly-placed candidates).
  - **complementarity** — strata by a **pre-registered controlled-feature or
    sampler-policy margin that is independent of every evaluated candidate
    metric** (register-occupancy / density fit), explicitly **not** called
    near/far: `wide_margin` / `narrow_margin` / `tie_margin`.
  - **copy-detection** — quotation-strength boundary strata:
    `obvious_copy_vs_remote` / `threshold_adjacent_copy_vs_noncopy` /
    `two_threshold_adjacent` (both candidates near the copy threshold).

  A metric that wins only on the wide-margin stratum (a riff against a musical
  refrigerator) has not passed; the gate is read **per stratum and in
  aggregate** (§2.6), so a wide-margin win cannot mask a narrow-margin or
  hard-negative loss. The margin bucketing is a pre-stratification device
  recorded in the item manifest; it is never revealed to the listener and
  never treated as the ground-truth label.
- **Stratification over corpus axes.** Beyond difficulty, items are stratified
  over the schema-v6 per-axis `ComplexityProfile` (rhythmic, pitch, technical,
  harmonic, playability — `core/src/structure.rs`) and over the
  structure/gesture facts and `SwancoreTag` categories (style / harmony /
  technique / rhythm / structure — `core/src/corpus.rs`), so no task is
  silently dominated by one region of the corpus (e.g. only clean single-note
  riffs).
- **Confound control (timbremetrics methodology).** When a task does not test a
  dimension, that dimension is controlled by matched sampling: for a similarity
  triplet not testing length, `B` and `C` are drawn in the same length band;
  likewise tempo and register. The *audible-rendering* confounds (timbre,
  loudness, mix balance) are controlled by the presentation profile (§2.3), not
  by sampling. The controlled dimensions per task are named in §1.
- **Source-identity discipline (the reachability-lab holdout law).** Any
  evaluation split is by **source identity (song), never by chunk** — a split
  that puts one bar of a song in train and the next in test measures
  recognition of the neighbouring bar, not musical understanding.
  Source-identity exclusion is enforced on the item set *before* triplets are
  assembled, exactly as the reachability lab enforces holdout before material
  construction.
- **Cross-user generalization needs its own split.** Held-out-user agreement
  (§2.6) requires a **user-disjoint** split — some users appear only in the
  held-out fold — and this split is **independent of the source-song split**
  (both hold simultaneously: a held-out-user, held-out-song evaluation is the
  strongest, and each split alone is insufficient for the other's claim). Both
  memberships are fixed by `dataset_split_id` (§2.2).
- **Same-song candidate pairs are a labelled special case.** A triplet whose
  `A` and a candidate share a song (unavoidable for §1.3, deliberate for some
  §1.4 copies) is **tagged as such in the item manifest**, never presented as a
  cross-song pair. Analysis includes or excludes same-song items explicitly,
  and a same-song win is never quietly counted as cross-song generalization.

### 2.5 Session procedure

- **Triplets per session (fatigue bound).** A session presents a bounded number
  of triplets so late judgements are not fatigue artefacts. Proposed bound:
  **≤ 30 triplets per session**, from listening-test hygiene (attention and
  consistency degrade over long forced-choice runs; ~20–40 comparisons is the
  usual working ceiling). The binding number is `TBD at spec` — decided by
  measuring per-user consistency (§2.6) as a function of within-session
  position on pilot data, and setting the bound where consistency starts to
  fall.
- **Order randomization.** `display_order` within each triplet, and triplet
  order within a session, are randomized per session (and the realized order is
  recorded, §2.2), so left/right and early/late position cannot masquerade as
  preference.
- **Repeated probes (per-user consistency).** A fraction of triplets are
  repeated within or across a user's sessions (identical content ids, re-drawn
  display order) to measure per-user self-agreement. A user whose repeat
  self-agreement is at chance contributes noise; their judgements are
  down-weighted or excluded under a documented rule. Repeated probes are the
  *same* item observed twice — the bootstrap (§2.6) must treat them as
  dependent, never as fresh independent evidence.
- **Minimum evidence threshold — aggregate *and* per stratum.** No task gate
  (§2.6) may be *read* until a floor of evidence exists, so a lucky handful of
  triplets cannot "pass" a task. Proposed aggregate floors: **≥ 50 triplets per
  user per task** and **≥ 5 distinct users** for any cross-user claim. But the
  gate is normative *per stratum* (§2.4, §2.6), and an aggregate floor cannot
  certify five underpopulated sub-gates by administrative osmosis — so **no
  per-stratum gate may be read until that stratum independently meets the
  pre-registered minimum user, source-group, and effective-item counts**
  (effective, i.e. after collapsing repeated probes within a cluster — §2.6).
  All these numbers are `TBD at spec`: the decision procedure fixes them from
  the target cluster-bootstrap CI width (§2.6) — the floor is whatever makes the
  CI lower bound a meaningful test rather than noise — not asserted by taste
  here.

### 2.6 The statistical gate, and which metric reads it

The gate is a **paired, per-item, non-inferiority test on a clustered
bootstrap** — not a comparison of two independently computed confidence
intervals. Comparing `lower_bound(candidate) ≥ lower_bound(baseline)` is
wrong: it can pass a system with a worse point estimate but smaller variance,
and it does not test superiority *on the same items*. The gate instead
bootstraps the **paired difference**.

**Per item `i`**, with the human choice as ground truth:

```text
Δ_i = correct(candidate, i) − correct(baseline, i)      // each ∈ {0, 0.5, 1}
                                                        // under the tie rule
                                                        // below, so Δ_i ∈ [−1, 1]
```

**Gate — two forms, one shape.** Against a real handcrafted baseline
(similarity, variation, copy-detection) the gate is **non-inferiority**:
`lower_CI(mean Δ) ≥ −δ`. Against a *floor* baseline (complementarity, where no
handcrafted distance exists — §1.3, §2.7) it is **superiority**:
`lower_CI(mean Δ) ≥ ε` with a **pre-registered `ε > 0`** — merely matching a
coin toss or a legality filter is not evidence, so the candidate must beat the
floor by a margin. In both:

- `δ` / `ε` are **pre-registered before collection** (never chosen after seeing
  results); `δ = 0` is the strict "no worse than baseline";
- the confidence level is fixed (default **95%**), as are the bootstrap
  **resample count** and **seed**, so the interval is reproducible;
- the **resampling unit is a cluster, not a row**: a **paired hierarchical /
  cluster bootstrap resamples over users and over source/song groups**, so the
  clustered structure of the data is preserved. A plain row bootstrap would let
  a thousand clicks from five people masquerade as a thousand independent
  listeners;
- **repeated probes** (§2.5) are collapsed within their cluster, never counted
  as independent draws;
- the gate is evaluated **per difficulty stratum (§2.4) and in aggregate** — a
  stratum failure is a failure even if the aggregate passes, and a stratum is
  read only once it meets its own evidence floor (§2.5).

**Prediction ties are handled deterministically and symmetrically — for
*either* predictor, never one arm.** A tie is when a predictor's two candidate
scores are equal (the `tie_margin` stratum, or any exact tie), so it makes no
prediction. One pre-registered rule, part of the gate-run manifest below,
applies to the candidate metric and the baseline alike:

- **default — half-credit:** a tie scores `correct = 0.5` for that predictor on
  that item (so `Δ_i` can be `±0.5`). Symmetric, and it never breaks the
  pairing.
- **alternative — paired abstention:** if *either* predictor ties on an item,
  the **whole paired item is removed from `Δ`** (never from just one arm —
  dropping one side would destroy `Δ_i` and bias the comparison). Chosen at
  spec; the two systems never run under different tie rules.

**The gate is a versioned run with its own identity, separate from the
judgement archive.** The judgement records (§2.2) deliberately bind **no** gate
policy — collection precedes, and is independent of, whichever model is
evaluated later, so the same evidence can be re-gated without rewriting a byte.
The statistical parameters live in their own immutable manifest, schema
`griff.similarity-benchmark-gate-run`, version `1`, which binds:

```text
gate_policy_id
judgement_archive_hash      // the exact evidence this gate ran over
tie_rule                    // half-credit | paired-abstention (above)
delta_or_epsilon            // δ for a real baseline, ε>0 for a floor
gate_form                   // non_inferiority | superiority | ungateable
confidence_level            // default 0.95
bootstrap_resample_count
bootstrap_seed
resampling_procedure_version   // the cluster/hierarchical procedure
per_stratum_evidence_floors    // min users / source-groups / effective-items
baseline_or_floor_id           // which §2.7 baseline/floor was compared
candidate_metric_id            // the evaluated system
```

A gate result cites a `gate_policy_id`; re-running the same `gate_policy_id`
over the same `judgement_archive_hash` reproduces the same verdict, or declares
a different policy. An unknown `griff.similarity-benchmark-gate-run` version is
a typed refusal, like every other bound identity.

**Metric-to-gate mapping (explicit, not implicit).**

| Metric | Role | Applies to |
| --- | --- | --- |
| Triplet agreement | **the gate metric** (paired-difference; non-inferiority vs a real baseline, superiority vs a floor — above) | all four tasks |
| Pairwise agreement (on *derived* labels, §2.2) | secondary diagnostic | similarity, variation |
| Per-user consistency | data-quality **precondition** read before any gate | all four |
| Cross-user generalization (user-disjoint split, §2.4) | generalization check, gated | all four; weighted heaviest for complementarity |
| Top-k retrieval, Kendall / Spearman, NDCG | **exploratory diagnostics, not gates in v1** | similarity (reported), `N/A` for copy-detection |
| Cluster bootstrap CI | the uncertainty envelope every gate reads | all four |

Two rules make the table binding:

- **Ranking metrics do not gate v1.** Top-k, Kendall/Spearman, and NDCG need a
  graded ranking of a candidate *pool* against `A`; the triplet protocol
  produces forced binary choices, not a pool ranking. They are computed and
  reported as diagnostics only. Promoting any of them to a gate requires a
  **separate versioned ranking-item protocol** (an item type that presents a
  pool and collects a ranking, with its own record fields) — named here as
  future work, not smuggled into the triplet gate.
- **The gate is the paired difference, clustered.** A point-estimate win inside
  overlapping per-arm intervals does not pass; only the paired-difference test
  on the clustered bootstrap does — non-inferiority (`≥ −δ`) against a real
  baseline, superiority (`≥ ε`, `ε > 0`) against a floor (§2.6).

### 2.7 Baselines and compared systems

The handcrafted baselines are **named code**, and "baseline distance" is
defined per task rather than assumed uniform. Learned embeddings and learned
distances enter later and, per the parent proposal's gate rule, must pass the
§2.6 gate *on that task* before earning any production use — non-inferiority
against a real baseline, or **superiority against a floor** where no baseline
exists (complementarity) — a win on one task is no licence on another (§1).

- **similarity** — baseline prediction = the candidate with the higher
  similarity aggregate to `A` under `similarity_weights_v3`
  (`core/src/similarity.rs`): the uniform-policy `WeightPolicy` over the named
  `SIMILARITY_AXIS_LABELS` (period / repeatability / loopability /
  structural-complexity / tags, the gesture axes, and the schema-v6 complexity
  axes). Reads persisted `ChunkMeta` metadata, **no note content**. Real
  baseline for measured chunks; inapplicable to unmeasured raw content (§1.1).
- **variation** — no single baseline metric exists; the baseline is a declared
  composite of the similarity aggregate (closeness) and `novelty.rs`
  `quote_novelty` / `ngram_novelty` (difference), peaking in a middle band
  whose breakpoints are `TBD at spec` (§1.2).
- **complementarity** — **honestly, no baseline distance.**
  `complement.rs::validate_pair` is a legality validator (and not a wired
  gate), producing `is_clean` rather than a graded preference. The gate
  therefore runs against a **declared floor** (random / legality-only) under the
  **superiority** form (`≥ ε`, `ε > 0`) — the learned score must *beat* the
  floor, not match it — or complementarity is declared ungateable until a
  meaningful handcrafted baseline exists; the choice is made at spec (§1.3,
  §2.6).
- **copy-detection** — baseline prediction = `novelty.rs` quotation facts
  (`quote_novelty`, `ngram_novelty`, longest common contiguous transition run),
  transposition- and resolution-robust by construction, predicting the copy as
  the lower-novelty candidate. Real baseline; the remote candidate is a control
  (§1.4), so no remoteness baseline is owed.

Compared systems, in the order the parent proposal admits them: handcrafted
symbolic distances (above); corpus features; learned embeddings (Step 1,
retrieval-only until they pass a gate); and combinations. Nothing learned
enters production scoring for a task until it clears that task's §2.6 gate —
the line this whole document exists to hold.

### 2.8 Collection surface

The intended surface is a new **A/B/C mode of the S8 playground**, which
already persists favorite/reject/history with typed provenance
(`ui-core/src/history.rs`; `cockpit/src/generation.rs`) — the benchmark reuses
that append-only, backend-neutral evidence spine and adds the triplet
presentation under a bound `PresentationProfile` (§2.3), the `skip`/`choice`
responses, session/user identity, the anchor + candidate audition timing of
§2.2, and the `A+B` / `A+C` mix playback for §1.3. **UI implementation is out
of scope of this proposal** — no mockups, no widget contracts, no `ui-core`
changes are specified or implied here; that surface earns its own spec section
and red→green slice through the S8 path when the parent proposal is accepted.

### 2.9 Non-goals

- No implementation, no API, no data-collection UI, and no delivery phase (the
  acceptance gate).
- No embeddings, no learned distance, and no training here — the benchmark
  *evaluates* them; Step-1 embeddings stay retrieval-only until they pass a
  gate.
- **No change to `rerank.rs`, to any frozen strategy, or to the S9 Phase 1 EMA
  plan, which remains the governing plan.** Acceptance of the *parent* proposal
  is what revises S9; this companion revises nothing.
- Benchmark results carry **no production authority** until the parent
  proposal's per-task gates run and it is accepted.
- **No audio similarity.** Griff is symbolic (MIDI in → MIDI out, no synthesis);
  no claim here concerns perceived *sound* or timbral/spectral distance.
  Timbre, loudness, and mix balance are nonetheless controlled as *presentation
  confounds* (§2.3) — controlled, not ignored, and not the object of any
  similarity claim.
- **No claim that agreement proves musical quality.** A metric that agrees with
  human similarity judgements agrees with human similarity judgements —
  nothing more. Quality, taste, and "is this a good riff" are out of scope;
  conflating agreement with quality is the error this non-goal names.

### 2.10 Prior art (idea-only; licences noted)

Surveyed before designing, per the prior-art-first rule; the survey is recorded
canonically in `docs/decisions.log.md` (dated entries), because a proposal is a
transient discussion artifact and the rule requires a durable trace outside it.
Idea reuse only — no code from any source below enters this MIT workspace.

- **timbremetrics / timbre-dissimilarity-metrics** — the human-similarity
  benchmark methodology this document transposes to symbolic riffs: triplet
  forced-choice agreement, top-k agreement, the Mantel test for
  matrix-vs-matrix correlation, and datasets built with **controlled
  confounds** (the discipline behind §2.4's matched sampling and difficulty
  strata). Method, not code.
- **Non-inferiority testing and the paired cluster / hierarchical bootstrap** —
  standard resampling and equivalence-testing practice, adopted for the §2.6
  gate: paired per-item differences, a pre-registered margin `δ`, and
  resampling over the clustering units (users, source songs) rather than rows.
  Applied as method; no library is added — the workspace reimplements the
  resampling natively (lean-dependency posture).
- **Pairwise-aggregation literature (Bradley–Terry / TrueSkill-style)** —
  referenced only for how *derived* pairwise labels (§2.2) could later be
  aggregated into a latent preference/skill score. Idea noted; no aggregation
  model is adopted — the benchmark stores observations and derives labels under
  a versioned policy, and any aggregation is a later decision.
- **Listening-test hygiene** — the fatigue bound, randomized presentation
  order, and repeated-probe consistency checks (§2.3, §2.5) follow standard
  forced-choice listening-test practice. Applied as protocol hygiene, not as
  any specific published apparatus.

The parent proposal's own prior-art section (PCGNN novelty decomposition,
learning-to-rank, MES/Bayesian optimization) governs the steps *around* the
benchmark; this document reuses those surveys by reference rather than
duplicating them.
