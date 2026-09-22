//! Small deterministic helpers shared by diagnostic-only corpus audits.

use std::cmp::Ordering;

use serde::Serialize;

/// A reduced, non-negative exact ratio.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub struct ExactRatio {
    /// Reduced numerator.
    pub numerator: u64,
    /// Reduced, non-zero denominator.
    pub denominator: u64,
}

impl ExactRatio {
    /// Reduces `numerator / denominator`; a zero denominator is represented as
    /// `numerator / 1` so malformed diagnostic metadata stays sortable.
    #[must_use]
    pub fn new(numerator: u64, denominator: u64) -> Self {
        let denominator = denominator.max(1);
        let divisor = gcd(numerator, denominator);
        Self {
            numerator: numerator / divisor,
            denominator: denominator / divisor,
        }
    }
}

impl Ord for ExactRatio {
    fn cmp(&self, other: &Self) -> Ordering {
        (u128::from(self.numerator) * u128::from(other.denominator))
            .cmp(&(u128::from(other.numerator) * u128::from(self.denominator)))
            .then_with(|| self.numerator.cmp(&other.numerator))
            .then_with(|| self.denominator.cmp(&other.denominator))
    }
}

impl PartialOrd for ExactRatio {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Integer/exact-ratio distribution using the nearest-rank definition for
/// median and percentiles (`ceil(p · n)`, one-based).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Distribution<T> {
    /// Number of observations.
    pub count: usize,
    /// Smallest observation.
    pub min: T,
    /// Nearest-rank p50.
    pub median: T,
    /// Nearest-rank p90.
    pub p90: T,
    /// Nearest-rank p95.
    pub p95: T,
    /// Nearest-rank p99.
    pub p99: T,
    /// Largest observation.
    pub max: T,
}

/// Summarizes a non-empty collection without floating-point quantiles.
#[must_use]
pub fn distribution<T: Clone + Ord>(values: &[T]) -> Option<Distribution<T>> {
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    Some(Distribution {
        count: sorted.len(),
        min: sorted.first()?.clone(),
        median: nearest_rank(&sorted, 50)?.clone(),
        p90: nearest_rank(&sorted, 90)?.clone(),
        p95: nearest_rank(&sorted, 95)?.clone(),
        p99: nearest_rank(&sorted, 99)?.clone(),
        max: sorted.last()?.clone(),
    })
}

/// Returns at most `n` items ordered by descending measure, then ascending
/// stable identity.
#[must_use]
pub fn top_n_longest<T, M, K>(
    mut values: Vec<T>,
    n: usize,
    measure: impl Fn(&T) -> M,
    identity: impl Fn(&T) -> K,
) -> Vec<T>
where
    M: Ord,
    K: Ord,
{
    values.sort_by(|a, b| {
        measure(b)
            .cmp(&measure(a))
            .then_with(|| identity(a).cmp(&identity(b)))
    });
    values.truncate(n);
    values
}

fn nearest_rank<T>(sorted: &[T], percentile: usize) -> Option<&T> {
    if sorted.is_empty() || !(1..=100).contains(&percentile) {
        return None;
    }
    let rank = percentile.saturating_mul(sorted.len()).div_ceil(100);
    sorted.get(rank.saturating_sub(1))
}

fn gcd(mut a: u64, mut b: u64) -> u64 {
    while b != 0 {
        let remainder = a % b;
        a = b;
        b = remainder;
    }
    if a == 0 {
        1
    } else {
        a
    }
}
