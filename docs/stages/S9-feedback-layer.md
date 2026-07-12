# S9: Human-feedback layer

Status: planned
Depends on: S6, S8
ADRs: —

## Goal

Let like/dislike/favorite steer ranking and sampling of candidates — not train
a model in a vacuum. Once basic preference learning is accepted, experiment with
structural parent selection, crossover, and mutation while retaining full
lineage and existing validators.

## Inputs / Outputs

- In: candidates + their feature vectors, user ratings.
- Out: a `PreferenceProfile`; reranked/resampled candidate sets.
- Later experiment: parent selections + structural evolution operators → a new
  candidate population carrying explicit lineage.

## Planned phases

### Phase 0 — feedback capture

Persist inspectable events with stable candidate/session identity:

```rust
pub struct HumanFeedback {
    pub candidate_id: CandidateId,
    pub verdict: FeedbackVerdict,
    pub generation: u32,
    pub session_id: SessionId,
}
```

The final names and storage format remain a design decision. `Skip`/no-op must be
representable so absence of a like is not silently treated as a dislike.

### Phase 1 — preference reranking

- Like/dislike/favorite → update feature weights.
- Baseline EMA update: `w_i ← (1-α)·w_i + α·sign(approve)·feature_i_norm`,
  `α ≈ 0.1`, weights normalized on the L1 simplex.
- Explainable rerank by similarity / features / tags.
- No gradient descent / RL before S10.

### Phase 2 — Evolution Lab (experiment)

Adapt only the human-in-the-loop idea from `perfect-shuffle-music`; do not port
its note-array genome or crossover implementation.

```text
candidate set
→ user selects parents
→ bar/motif/parameter crossover
→ mutation
→ meter/register/playability/novelty validators
→ existing rerank as a safety/quality guard
→ next generation
```

Candidate operator vocabulary:

```rust
pub enum EvolutionOperator {
    AlternateBars,
    AlternateMotifs,
    RhythmFromAContourFromB,
    PrefixSuffixCrossover,
    ParameterBlend,
}
```

Lineage records generation, parents, operator, mutations, and session. Musical
units are bars, motifs, rhythm grids, contours, gesture plans, register windows,
endings, and strategy parameters — never a blind alternating array of MIDI note
indices.

### Phase 3 — diversity and collapse controls

Measure and expose:

- population and strategy diversity;
- rhythm/contour diversity;
- repeated-parent dominance;
- operator survival rates;
- population collapse across generations.

Evolution Lab remains an S9 experiment. A new stage is justified only if it
becomes a standalone persistent workflow with branching histories, undo/fork,
operator analytics, and its own acceptance contract.

## Acceptance criteria

- Ratings measurably shift subsequent ranking toward liked features
  (deterministic test with synthetic ratings).
- The profile is inspectable and resettable.
- Feedback events retain stable candidate/session provenance.
- Evolution operators preserve structural validity and pass the normal generator
  validators.
- Fixed inputs, feedback sequence, and seed reproduce the same population and
  lineage.
- Diversity/collapse metrics are reported before Evolution Lab is considered for
  product use.

## Open questions

- Per-tag Beta-prior alternative vs single EMA.
- Minimal parent-selection UX and population size.
- Whether lineage belongs in the corpus schema or an experiment/session store.

## See also

- [`../audit/2026-07-symbolic-harmony-and-evolution-research.md`](../audit/2026-07-symbolic-harmony-and-evolution-research.md)
- [`S7-graph-layer.md`](S7-graph-layer.md) — deterministic k-best global alternatives
- [`S8-preview-app.md`](S8-preview-app.md) — feedback and lineage UI
- [`S15-tonal-context-and-harmonic-control.md`](S15-tonal-context-and-harmonic-control.md)
- [`../glossary.md`](../glossary.md) §10
