//! Unified explainable scoring vocabulary (ADR-0017).
//!
//! Every score in `griff` — phrase-boundary likelihood (S4), the complement
//! relation (S13), structure metrics (S14), the DP cost function (S7), the
//! quality score — is the same shape described four different ways. This module
//! fixes one shape so they share a vocabulary, a tuning surface, and a UI:
//!
//! - [`Axes`] — the named, per-axis measurements (the *facts*). Carried as
//!   labelled data, not a fixed struct, so different domains reuse the shape.
//! - [`WeightPolicy`] — a named, *versioned* weighting (the *policy*). Weights
//!   are data the feedback layer (S9) tunes; they are never hardcoded into the
//!   code that computes axes (ADR-0017 §3, generalising ADR-0013 §4).
//! - [`Rationale`] — the explainable trace: per-axis value, weight, and
//!   contribution (the "why"). Distinct from import-side *evidence*.
//! - the **aggregate** — [`Scored::aggregate`], a *derived* scalar
//!   (`Σ value·weight`), never the source of truth (the anti-scalar rule,
//!   ADR-0017 §2).
//! - [`Provenance`] — what makes the aggregate reproducible: the weight-policy
//!   version, plus the seed where RNG applied (ADR-0017 §7).
//!
//! [`rank_indices`] gives a total, stable ordering (aggregate descending, ties
//! broken by ascending index) so candidate selection is deterministic under a
//! fixed `(seed, weight-policy version)` (ADR-0017 §7, extends SPEC §6).

use std::slice::Iter;

/// A single named per-axis measurement — a scoring *fact*.
///
/// Rank axes are normalised to `[0, 1]`; signed cost axes (ADR-0013) may fall
/// outside it. The value is a measurement, not a verdict; the verdict is the
/// derived aggregate under a [`WeightPolicy`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Axis {
    /// Stable axis label, used as the join key against a [`WeightPolicy`].
    pub label: &'static str,
    /// The measured value on this axis.
    pub value: f64,
}

/// The ordered set of per-axis facts behind a score (ADR-0017).
///
/// Labelled data rather than a fixed struct, so boundary, complement, structure,
/// and cost scores all share the shape.
#[derive(Debug, Clone, Default)]
pub struct Axes(Vec<Axis>);

impl Axes {
    /// Builds an axis set from an ordered list of axes.
    #[must_use]
    pub const fn new(axes: Vec<Axis>) -> Self {
        Self(axes)
    }

    /// The value on `label`, or `None` when the axis is absent.
    #[must_use]
    pub fn get(&self, label: &str) -> Option<f64> {
        self.0.iter().find(|a| a.label == label).map(|a| a.value)
    }

    /// Iterates the axes in their stored order.
    pub fn iter(&self) -> Iter<'_, Axis> {
        self.0.iter()
    }

    /// The number of axes.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.0.len()
    }

    /// Whether the axis set is empty.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl<'a> IntoIterator for &'a Axes {
    type Item = &'a Axis;
    type IntoIter = Iter<'a, Axis>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

/// A named, versioned weighting policy — the surface the feedback layer (S9)
/// tunes (ADR-0017 §3).
///
/// Weights are *data*: kept separate from the code that computes axes, so S9 can
/// adjust them without touching scoring, and different relation modes / affects
/// are different policies rather than branches in the scorer. A score is only
/// reproducible relative to a policy `(id, version)`, which travels in
/// [`Provenance`].
#[derive(Debug, Clone)]
pub struct WeightPolicy {
    /// Stable policy identifier (e.g. `"relation"`).
    pub id: &'static str,
    /// Policy version; bumped when the weights change.
    pub version: u32,
    /// Per-axis weights, keyed by axis label.
    weights: Vec<(&'static str, f64)>,
}

impl WeightPolicy {
    /// Builds a policy from explicit per-axis weights.
    #[must_use]
    pub const fn new(id: &'static str, version: u32, weights: Vec<(&'static str, f64)>) -> Self {
        Self {
            id,
            version,
            weights,
        }
    }

    /// Builds a uniform policy — each of `labels` weighted `1/n` — so an
    /// all-`1.0` axis set aggregates to `1.0`. A neutral, untuned baseline.
    #[must_use]
    #[allow(clippy::cast_precision_loss)] // axis count is tiny; no precision concern
    pub fn uniform(id: &'static str, version: u32, labels: &[&'static str]) -> Self {
        let weight = if labels.is_empty() {
            0.0
        } else {
            1.0 / labels.len() as f64
        };
        Self {
            id,
            version,
            weights: labels.iter().map(|&label| (label, weight)).collect(),
        }
    }

    /// The weight on `label`. Axes outside the policy contribute nothing
    /// (weight `0.0`), so an unknown axis cannot silently sway the aggregate.
    #[must_use]
    pub fn weight(&self, label: &str) -> f64 {
        self.weights
            .iter()
            .find(|(l, _)| *l == label)
            .map_or(0.0, |(_, w)| *w)
    }
}

/// One line of the explainable trace: an axis, its value, the weight applied,
/// and the resulting contribution to the aggregate (ADR-0017).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RationaleEntry {
    /// The axis this line explains.
    pub axis: &'static str,
    /// The measured value on the axis.
    pub value: f64,
    /// The weight the policy applied to the axis.
    pub weight: f64,
    /// `value · weight` — this axis's share of the aggregate.
    pub contribution: f64,
}

/// The explainable trace behind a score: one [`RationaleEntry`] per axis.
#[derive(Debug, Clone, Default)]
pub struct Rationale(Vec<RationaleEntry>);

impl Rationale {
    /// The rationale entries, in axis order.
    #[must_use]
    pub fn entries(&self) -> &[RationaleEntry] {
        &self.0
    }
}

/// Output metadata that makes an aggregate reproducible (ADR-0017 §7).
///
/// A stored score without its weight-policy version is not reproducible (the
/// policy may have been retuned by S9 since), so the version travels with the
/// score, alongside the seed where RNG applied.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Provenance {
    /// Identifier of the weight policy the score was computed under.
    pub policy_id: &'static str,
    /// Version of that weight policy.
    pub policy_version: u32,
    /// The RNG seed, where generation used one (`None` for purely analytic scores).
    pub seed: Option<u64>,
}

/// A produced value together with its score: facts, rationale, and provenance
/// (ADR-0017 §6).
///
/// Replaces ad-hoc `{ score, reason }` shapes so every scored thing —
/// candidate, boundary, part — carries the same explainable envelope. The
/// aggregate is *derived* from `axes` and a policy, never stored as the truth.
#[derive(Debug, Clone)]
pub struct Scored<T> {
    /// The produced value (or a light locator for it).
    pub value: T,
    /// The per-axis facts.
    pub axes: Axes,
    /// The explainable trace under the scoring policy.
    pub rationale: Rationale,
    /// Reproducibility metadata.
    pub provenance: Provenance,
}

impl<T> Scored<T> {
    /// Scores `value`'s `axes` under `policy`, building the rationale and
    /// provenance. `seed` records the RNG seed where one was used.
    ///
    /// The aggregate is not stored; it is recomputed from the rationale by
    /// [`Scored::aggregate`], keeping the axes the single source of truth.
    #[must_use]
    pub fn new(value: T, axes: Axes, policy: &WeightPolicy, seed: Option<u64>) -> Self {
        let entries = axes
            .iter()
            .map(|a| {
                let weight = policy.weight(a.label);
                RationaleEntry {
                    axis: a.label,
                    value: a.value,
                    weight,
                    contribution: a.value * weight,
                }
            })
            .collect();
        Self {
            value,
            axes,
            rationale: Rationale(entries),
            provenance: Provenance {
                policy_id: policy.id,
                policy_version: policy.version,
                seed,
            },
        }
    }

    /// The derived aggregate: the sum of per-axis contributions (ADR-0017 §1).
    #[must_use]
    pub fn aggregate(&self) -> f64 {
        self.rationale
            .entries()
            .iter()
            .map(|e| e.contribution)
            .sum()
    }
}

/// Returns indices into `scored` ordered by **descending aggregate**, ties
/// broken by **ascending original index** — a total, stable order (ADR-0017 §7).
///
/// The fixed tie-break makes candidate selection reproducible relative to the
/// `(seed, weight-policy version)` the scores were computed under, satisfying
/// SPEC §6 without an RNG in the ranking.
#[must_use]
pub fn rank_indices<T>(scored: &[Scored<T>]) -> Vec<usize> {
    let mut keyed: Vec<(usize, f64)> = scored
        .iter()
        .enumerate()
        .map(|(i, s)| (i, s.aggregate()))
        .collect();
    keyed.sort_by(|(ai, av), (bi, bv)| bv.total_cmp(av).then(ai.cmp(bi)));
    keyed.into_iter().map(|(i, _)| i).collect()
}
