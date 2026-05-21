//! Human-feedback layer (S9).
//!
//! Maintains a [`PreferenceProfile`] — a weight vector on the L1 simplex —
//! updated by EMA when the user rates generation candidates.  Provides
//! [`rerank`] to sort feature vectors by profile score (descending).

use crate::graph::NodeFeatureVec;

// ── rating ────────────────────────────────────────────────────────────────────

/// A user rating applied to a generated candidate.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Rating {
    /// Positive signal; shift weights toward these features.
    Like,
    /// Negative signal; shift weights away from these features.
    Dislike,
    /// Strong positive signal; same direction as `Like` but with doubled α.
    Favorite,
}

// ── preference profile ────────────────────────────────────────────────────────

/// Feature weights on the L1 simplex, updated by EMA from user ratings.
///
/// Component `w_i` represents how much the user values feature dimension `i`.
/// All weights are non-negative and sum to 1.0 (uniform at construction).
#[derive(Debug, Clone)]
pub struct PreferenceProfile {
    /// Non-negative weights summing to 1.0.
    pub weights: Vec<f64>,
    /// EMA learning rate α (default 0.1).
    pub alpha: f64,
}

impl PreferenceProfile {
    /// Creates a uniform profile over `dim` feature dimensions with α = 0.1.
    pub fn new(dim: usize) -> Self {
        Self::with_alpha(dim, 0.1)
    }

    /// Creates a uniform profile with a custom learning rate.
    pub fn with_alpha(dim: usize, alpha: f64) -> Self {
        let weight = uniform_weight(dim);
        Self {
            weights: vec![weight; dim],
            alpha,
        }
    }

    /// Applies one EMA update:
    ///
    /// `w_i ← (1 − α_eff) · w_i + α_eff · sign · feature_i`
    ///
    /// where `sign` is +1 for `Like`/`Favorite` and −1 for `Dislike`, and
    /// `α_eff` is `α` (Like/Dislike) or `min(2α, 1)` (Favorite).
    ///
    /// After the update the weights are clamped to `[0, ∞)` and
    /// re-normalised onto the L1 simplex.  If all weights become zero
    /// (e.g. extreme dislike) the profile resets to uniform.
    pub fn update(&mut self, features: &NodeFeatureVec, rating: Rating) {
        let eff_alpha = match rating {
            Rating::Favorite => (2.0_f64 * self.alpha).min(1.0_f64),
            _ => self.alpha,
        };
        let sign: f64 = match rating {
            Rating::Dislike => -1.0,
            _ => 1.0,
        };
        let complement = 1.0_f64 - eff_alpha;

        for (w, &fv) in self.weights.iter_mut().zip(features.values.iter()) {
            *w = complement.mul_add(*w, eff_alpha * sign * fv);
        }
        project_l1(&mut self.weights);
    }

    /// Returns the dot product of the profile weights and `features`.
    pub fn score(&self, features: &NodeFeatureVec) -> f64 {
        self.weights
            .iter()
            .zip(features.values.iter())
            .map(|(&w, &fv)| w * fv)
            .sum()
    }

    /// Resets all weights to uniform (does not change α or dimension).
    pub fn reset(&mut self) {
        let w = uniform_weight(self.weights.len());
        for weight in &mut self.weights {
            *weight = w;
        }
    }
}

// ── rerank ────────────────────────────────────────────────────────────────────

/// Returns indices of `feature_vecs` sorted by descending profile score.
///
/// The output is a permutation of `0..feature_vecs.len()`.
pub fn rerank(feature_vecs: &[NodeFeatureVec], profile: &PreferenceProfile) -> Vec<usize> {
    let mut scored: Vec<(usize, f64)> = feature_vecs
        .iter()
        .enumerate()
        .map(|(i, fv)| (i, profile.score(fv)))
        .collect();
    scored.sort_unstable_by(|(_, sa), (_, sb)| sb.total_cmp(sa));
    scored.into_iter().map(|(i, _)| i).collect()
}

// ── private helpers ───────────────────────────────────────────────────────────

/// Returns `1/dim`, or `0.0` when `dim` is zero.
fn uniform_weight(dim: usize) -> f64 {
    let n = u32::try_from(dim).unwrap_or(u32::MAX);
    if n == 0 {
        0.0
    } else {
        1.0_f64 / f64::from(n)
    }
}

/// Clamps `weights` to `[0, ∞)` and normalises to sum 1.0.
///
/// If all weights are zero after clamping, resets to uniform.
fn project_l1(weights: &mut [f64]) {
    for w in weights.iter_mut() {
        *w = w.max(0.0_f64);
    }
    let sum: f64 = weights.iter().sum();
    if sum > 0.0_f64 {
        for w in weights.iter_mut() {
            *w /= sum;
        }
    } else {
        let u = uniform_weight(weights.len());
        for w in weights.iter_mut() {
            *w = u;
        }
    }
}
