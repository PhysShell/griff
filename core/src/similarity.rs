//! Chunk similarity v1 — the first S7 edge, over persisted corpus axes.
//!
//! Computes the *similarity* edge of the graph layer (S7, glossary §9) between
//! corpus chunks, using only facts already persisted in `ChunkMeta`: the
//! [`StructureMetrics`] that S14 Phase 3 wrote into the schema (v2) and the
//! swancore tag set. No note content is read — the manifest carries none — so
//! retrieval works straight off the corpus file.
//!
//! Shape per the 2026-06-10 `AudioMuse` prior-art decision (idea (a)): a
//! brute-force pass over *named* symbolic feature axes with a per-axis
//! rationale (ADR-0017), not an ANN index — the corpus is micro-scale and an
//! explainable edge is the point. Each axis is an agreement measure in
//! `[0, 1]`:
//!
//! - `period_similarity` — min/max ratio of the detected pattern periods in
//!   bars; two through-composed chunks agree (`1.0`), a periodic chunk against
//!   a through-composed one does not (`0.0`).
//! - `repeatability_similarity` / `loopability_similarity` /
//!   `complexity_similarity` — `1 − |Δ|` on the corresponding
//!   [`StructureMetrics`] scalars. `variation_score` is deliberately not an
//!   axis: it is `1 − repeatability` by construction, and a duplicate axis
//!   would silently double-weight the same fact.
//! - `tag_similarity` — Jaccard over the [`SwancoreTag`] sets; two untagged
//!   chunks agree (the empty-set convention shared with
//!   `structure::set_jaccard`).
//!
//! [`find_similar_chunks`] ranks candidates against a query under a versioned
//! [`WeightPolicy`] via the shared [`Scored`] envelope, so every neighbour
//! carries its per-axis rationale and the ranking is reproducible relative to
//! the policy version (ADR-0017 §7). Unmeasured records (schema-v1, no
//! `structure` key) cannot sit on this edge: an unmeasured *query* is an
//! error, unmeasured *candidates* are skipped until re-curated. Pure and
//! deterministic (SPEC §6).

use std::collections::HashSet;

use crate::corpus::{ChunkId, ChunkMeta, SwancoreTag};
use crate::scoring::{rank_indices, Axes, Axis, Scored, WeightPolicy};
use crate::structure::StructureMetrics;

// Stable axis labels — the join keys between [`similarity_axes`] facts and a
// scoring [`WeightPolicy`] (ADR-0017). Named once so the labels and the axis
// order cannot drift apart.
const AXIS_PERIOD: &str = "period_similarity";
const AXIS_REPEATABILITY: &str = "repeatability_similarity";
const AXIS_LOOPABILITY: &str = "loopability_similarity";
const AXIS_COMPLEXITY: &str = "complexity_similarity";
const AXIS_TAGS: &str = "tag_similarity";

/// The similarity axes, in their canonical order (ADR-0017).
pub const SIMILARITY_AXIS_LABELS: [&str; 5] = [
    AXIS_PERIOD,
    AXIS_REPEATABILITY,
    AXIS_LOOPABILITY,
    AXIS_COMPLEXITY,
    AXIS_TAGS,
];

/// Errors chunk-similarity retrieval can emit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SimilarityError {
    /// The query chunk carries no measured structure (a schema-v1 record);
    /// re-curate it before asking for its neighbours.
    QueryUnmeasured,
}

/// The similarity facts between two measured chunks, as labelled axes in
/// `[0, 1]` (ADR-0017) — higher is more alike; symmetric in its arguments.
///
/// Returns `None` when either side is unmeasured (no persisted
/// [`StructureMetrics`]): an absent fact is not a zero-similarity fact.
#[must_use]
pub fn similarity_axes(a: &ChunkMeta, b: &ChunkMeta) -> Option<Axes> {
    let ma = a.structure.as_ref()?;
    let mb = b.structure.as_ref()?;

    Some(Axes::new(vec![
        Axis {
            label: AXIS_PERIOD,
            value: period_similarity(ma, mb),
        },
        Axis {
            label: AXIS_REPEATABILITY,
            value: scalar_similarity(ma.repeatability_score, mb.repeatability_score),
        },
        Axis {
            label: AXIS_LOOPABILITY,
            value: scalar_similarity(ma.loopability_score, mb.loopability_score),
        },
        Axis {
            label: AXIS_COMPLEXITY,
            value: scalar_similarity(ma.structural_complexity, mb.structural_complexity),
        },
        Axis {
            label: AXIS_TAGS,
            value: tag_similarity(&a.tags, &b.tags),
        },
    ]))
}

/// The baseline similarity weight policy (`similarity` v1): uniform over the
/// five axes. Untuned by design — weights are data the feedback layer (S9)
/// learns (ADR-0017 §3).
#[must_use]
pub fn similarity_weights_v1() -> WeightPolicy {
    WeightPolicy::uniform("similarity", 1, &SIMILARITY_AXIS_LABELS)
}

/// Ranks `candidates` by similarity to `query` under `policy`, most similar
/// first — the first S7 edge, queried brute-force.
///
/// The query itself (matched by [`ChunkId`]) and unmeasured candidates are
/// excluded; each neighbour returns as a [`Scored`] envelope over its
/// [`ChunkId`] with the per-axis rationale. Ties break by candidate order
/// (the fixed [`rank_indices`] rule), so the ranking is deterministic for a
/// fixed input order and policy version (ADR-0017 §7).
pub fn find_similar_chunks(
    query: &ChunkMeta,
    candidates: &[ChunkMeta],
    policy: &WeightPolicy,
) -> Result<Vec<Scored<ChunkId>>, SimilarityError> {
    if query.structure.is_none() {
        return Err(SimilarityError::QueryUnmeasured);
    }

    let scored: Vec<Scored<ChunkId>> = candidates
        .iter()
        .filter(|c| c.id != query.id)
        .filter_map(|c| {
            let axes = similarity_axes(query, c)?;
            Some(Scored::new(c.id.clone(), axes, policy, None))
        })
        .collect();

    let order = rank_indices(&scored);
    let mut slots: Vec<Option<Scored<ChunkId>>> = scored.into_iter().map(Some).collect();
    Ok(order
        .into_iter()
        .filter_map(|i| slots.get_mut(i).and_then(Option::take))
        .collect())
}

/// Min/max ratio of two detected pattern periods (bars): equal periods score
/// `1.0`, an off-by-2× period `0.5`. Two through-composed chunks agree;
/// periodic-vs-through-composed is fully dissimilar.
#[allow(clippy::cast_precision_loss)] // bar periods are tiny; no precision concern
fn period_similarity(a: &StructureMetrics, b: &StructureMetrics) -> f64 {
    match (
        a.detected_pattern_period_bars,
        b.detected_pattern_period_bars,
    ) {
        (None, None) => 1.0,
        (None, Some(_)) | (Some(_), None) => 0.0,
        (Some(pa), Some(pb)) => {
            let lo = pa.min(pb);
            let hi = pa.max(pb);
            if hi == 0 {
                0.0
            } else {
                lo as f64 / hi as f64
            }
        }
    }
}

/// `1 − |a − b|` over unit-range scalars, clamped against malformed records.
fn scalar_similarity(a: f64, b: f64) -> f64 {
    1.0 - (a - b).abs().clamp(0.0, 1.0)
}

/// Jaccard similarity of two tag sets; duplicates count once, and two
/// untagged chunks read as identical (`1.0`).
#[allow(clippy::cast_precision_loss)] // tag counts are tiny; no precision concern
fn tag_similarity(a: &[SwancoreTag], b: &[SwancoreTag]) -> f64 {
    let sa: HashSet<SwancoreTag> = a.iter().copied().collect();
    let sb: HashSet<SwancoreTag> = b.iter().copied().collect();
    let union = sa.union(&sb).count();
    if union == 0 {
        return 1.0;
    }
    let inter = sa.intersection(&sb).count();
    inter as f64 / union as f64
}
