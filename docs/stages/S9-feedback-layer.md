# S9: Human-feedback layer

Status: planned
Depends on: S6, S8
ADRs: —

## Goal

Let like/dislike/favorite steer ranking and sampling of candidates — not train
a model in a vacuum.

## Inputs / Outputs

- In: candidates + their feature vectors, user ratings.
- Out: a `PreferenceProfile`; reranked/ resampled candidate sets.

## Approach

- Like/dislike/favorite → update feature weights.
- Baseline EMA update: `w_i ← (1-α)·w_i + α·sign(approve)·feature_i_norm`,
  `α ≈ 0.1`, weights normalized on the L1 simplex.
- Explainable rerank by similarity / features / tags.
- No gradient descent / RL before S10.

## Acceptance criteria

- Ratings measurably shift subsequent ranking toward liked features
  (deterministic test with synthetic ratings).
- The profile is inspectable and resettable.

## Open questions

- Per-tag Beta-prior alternative vs single EMA.

## See also

- [`../glossary.md`](../glossary.md) §10
