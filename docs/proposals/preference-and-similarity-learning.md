# Proposal: Staged preference and similarity learning

The order in which ML is allowed into Griff: evidence first, retrieval-only
embeddings second, a bounded linear reranker third — each gated by a human
benchmark, none with authority over legality.

Status: proposal for discussion (v1 — distilled from the 2026-07 research
memos on generation, optimization and ML)
Scope: docs-only until accepted; binds nothing. Feeds S9 (feedback layer)
and complements the Generator Reachability Lab proposal. `rerank.rs` and
every frozen strategy stay untouched until an explicit acceptance contract.

## 1. Architecture position

Griff stays hybrid; ML slots into exactly one box:

```text
intent / control curves → hierarchical generation → hard constraints
  → valid candidate space → handcrafted + learned scoring → DP / k-best
  → candidate sequence → human choice → preference evidence (loop)
```

Constraints define the legal. Generators create variants. Scoring ranks the
legal. Search assembles globally coherent sequences (already confirmed in
Griff: the locally best candidate is not always on the globally best path).
Humans check whether the scoring means anything. Each method answers a
different question; they are not competitors:

| question | method |
| --- | --- |
| strict admissibility | hard constraints / bounded solver (see the constraint-contract proposal) |
| best path over a known candidate graph | DP / k-best (ADR-0013/0030) — stays the decision-maker |
| rank candidates from likes/rejects | pairwise ranking, gradient descent |
| tune few continuous params on a cheap metric | ARS / CMA-ES |
| tune params on scarce human ratings | Bayesian optimization / MES |
| representations without labels | self-supervised learning |
| pick per-context among few candidates | contextual bandit (later) |
| delayed-reward sequential decisions | MDP/RL (much later) |
| coordinating autonomous parts | MARL (much, much later) |

## 2. Stage 0 — preference evidence (no ML)

S8 already emits the raw material: favorite / reject / history with full
candidate provenance (Swang Playground Slice 3, PR #126). Stage 0 is a data
contract, not a model: persist each judgement as `(context, candidate A,
candidate B or set, choice, provenance, session identity)` so that pairwise
labels can be *derived* later. The scarce resource in this project is human
listening, not CPU (reachability-lab Phase 0 premise) — evidence collection
must therefore be designed before any learner exists.

## 3. Stage 1 — symbolic SSL embeddings, retrieval-only

Self-supervised tasks over the corpus (no manual labels): masked-event
prediction, temporal-order prediction, transposition consistency,
equivalent-fingering consistency, contrastive motif learning, local
continuation, phrase-boundary prediction (S4 gives free labels).

Allowed uses while unproven: near-duplicate detection, motif retrieval,
cluster validation, corpus anomaly detection, diversity selection.
Forbidden use while unproven: any influence on production scoring.

Hard split rule (same law as the reachability lab's holdout): train/val/test
split by **source identity** (song), never by chunk — otherwise the
benchmark measures recognition of the neighbouring bar, not musical
understanding.

## 4. Stage 2 — Human Similarity Benchmark (the gate)

Adopted from timbremetrics methodology, transposed to symbolic riffs.
Anchor A, candidates B/C; questions: more similar to A? better variation of
A? better complement to A? too close (a copy)? too far (unrelated)?

Metrics: triplet agreement, pairwise agreement, top-k retrieval, rank
correlation (Kendall/Spearman), NDCG, per-user consistency, cross-user
generalisation, bootstrap confidence intervals. Compared systems:
handcrafted symbolic distances (the existing `similarity.rs` /
`novelty.rs` vocabulary), corpus features, learned embeddings, combinations.

**Gate rule: no embedding or learned distance enters production scoring
until it beats or matches the handcrafted baseline on this benchmark.**
Cosine distance is not human perception until proven so.

## 5. Stage 3 — linear pairwise reranker, bounded correction

First learner: standardized features (harmonic fit, rhythmic fit, pitch/fret
movement, density, repetition, novelty, playability, register, technique
usage, phrase role, transition cost — largely the existing explainable
axes), linear model `S(x) = wᵀf(x) + b`, pairwise logistic loss on derived
A-over-B labels, L2 regularization, source-disjoint split, multiple seeds,
always compared against the handcrafted scorer.

Integration is a bounded correction, never a replacement:

```text
S_final = S_manual + α · S_learned      // α starts small
```

"Bounded" is a calibration contract, not a hope about α: `S_learned` is
calibrated to a fixed scale before mixing (standardized to zero mean and
unit variance over a versioned calibration set of admissible candidates,
then clipped to a documented range), so that α alone controls the maximum
correction relative to the documented `S_manual` scale. α starts in a small
explicit range (e.g. `(0, 0.25]`) and both the calibration parameters and α
are part of the versioned scoring policy. Every α change reports an
ordering-shift measurement (fraction of top-1 and top-k changes on a fixed
evaluation set), so the learned term demonstrably remains a correction
rather than silently dominating the ranking through scale or offset.

The DP layer still assembles the path; the learner only adjusts local scores.
A small MLP (features → 32 → 16 → score; AdamW, lr 3e-4..3e-3, weight decay
0..1e-3, batch 32–64, 3–5 seeds, early stopping) is admitted **only** after
the linear model demonstrably saturates on interaction effects. Excluded
until a measured problem demands them: BatchNorm, transformers, schedulers
beyond early stopping, architecture zoo.

Experiment discipline (funnel, per hypothesis): formal hypothesis → simple
baseline → one change group → fixed dataset and evaluation budget → several
seeds → held-out evaluation → accept or roll back. Metrics: pairwise
agreement, top-1 agreement, NDCG, user top-1 replacements, per-context
splits (clean/heavy/transition), seed stability. If loss improves and the
music gets worse, the objective/labels/data are wrong — the optimizer just
got there faster.

## 6. Later stages (ordered, all evidence-gated)

1. **MES / Bayesian optimization** for 8–20 interpretable generator
   parameters once favorite/reject history is dense enough — human ratings
   are expensive, so pick configurations by information gain, with cheap
   (constraints, playability, repetition, corpus distance), medium (learned
   score, embedding novelty) and expensive (human choice) evaluation tiers.
2. **Contextual bandit** for choosing a candidate family per context — the
   likely first *online* learner, since choices are near-independent.
3. **Multi-track counterfactual scoring** before any MARL thought: store
   local score A, local score B, joint score, interaction term, and
   counterfactual credit `score(A,B) − mean_R score(A,R)` — "does B actually
   complement A, or is it just independently decent?" (S13 territory.)
4. **ARS/CMA-ES** for handcrafted-weight calibration where a cheap metric
   exists and gradients don't; offline multi-objective pipeline search
   (rhythm source × pitch policy × selection × transition) as a research
   tool, never a runtime.

## 7. Non-goals

Starting with a transformer, RL, or MARL; a learned symbolic generator or
latent terrain before the benchmark exists; embeddings as ground truth;
learned models with authority over validity, timing, export, provenance, or
acceptance gates; novelty mistaken for quality; any change to `rerank.rs`
or frozen strategies without a separate acceptance contract.

## 8. Prior art surveyed (prior-art-first rule, AGENTS.md)

timbremetrics / timbre-dissimilarity-metrics (human-similarity benchmark
methodology: triplet agreement, Mantel test, top-k agreement, 21 datasets
with controlled confounds); PCGNN (diversity decomposition: intra-batch,
archive, source-relative, axis-controlled novelty — metrics adopted, NEAT
not); learning-to-rank literature (pairwise logistic / RankNet-style loss);
max-value entropy search and standard BO for expensive black-box human
evaluation; value decomposition and counterfactual credit from the MARL
literature as *bookkeeping* ideas only. The equivalent-parameterization
problem (Ben Hayes: many synth configs, one sound) maps to Griff's many
fingerings / one note — canonicalize before comparing or deduplicating.
