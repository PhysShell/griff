//! Metrics and when two of them may be compared.

use crate::fingerprint::Fingerprint;
use crate::spec::PolicyIdentity;

/// The evaluator of [`crate::EvaluationContext::GenerationAxes`]: closure then
/// novelty axes of a result's first track.
pub const EVALUATOR_GENERATION_AXES: PolicyIdentity = PolicyIdentity {
    id: "generation_axes",
    version: 1,
};

/// What a metric measures against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MetricKind {
    /// Measured by a fixed evaluator in the spec's fixed evaluation context —
    /// the only kind an interaction is defined over.
    Evaluation,
    /// A policy's own number under the inputs of the pass that produced it.
    PolicyObjective,
}

/// Everything that decides whether two values live on one scale: what kind of
/// metric, which axis, whose evaluator or policy at which version, and the
/// fingerprint of the context it was measured in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MetricIdentity {
    /// Evaluation or policy objective.
    pub kind: MetricKind,
    /// The axis or objective name.
    pub name: &'static str,
    /// The evaluator or policy that measured it.
    pub owner: PolicyIdentity,
    /// The context it was measured in: the evaluation context for an
    /// evaluation, the pass's information for a policy objective.
    pub context: Fingerprint,
}

/// One measured value and its identity.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MetricValue {
    /// When this value may be compared with another.
    pub identity: MetricIdentity,
    /// The value.
    pub value: f64,
}

/// Why a comparison has no number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unavailable {
    /// A side has no such metric (no evaluator, or a refused cell).
    Missing,
    /// The values live on different scales: another metric, owner, version,
    /// or context.
    IncompatibleIdentity,
    /// An interaction was asked of something that is not an evaluation.
    NotAnEvaluation,
}

/// A comparison's outcome: a number, or the reason there is none.
// A number and a one-byte reason differ in size by nature; boxing the number
// to satisfy the lint would buy nothing.
#[allow(variant_size_differences)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Comparison {
    /// The arithmetic, over compatible identities.
    Available(f64),
    /// No meaningful number exists.
    Unavailable(Unavailable),
}

/// `to − from`, when both exist and share one identity.
#[must_use]
pub fn delta(from: Option<&MetricValue>, to: Option<&MetricValue>) -> Comparison {
    match (from, to) {
        (Some(from), Some(to)) if from.identity == to.identity => {
            Comparison::Available(to.value - from.value)
        }
        (Some(_), Some(_)) => Comparison::Unavailable(Unavailable::IncompatibleIdentity),
        _ => Comparison::Unavailable(Unavailable::Missing),
    }
}

/// `(b1 − a1) − (b0 − a0)`: how much variant B's effect over A changes between
/// two regimes. Defined only when all four are evaluations with one identity.
#[must_use]
pub fn interaction(
    a0: Option<&MetricValue>,
    b0: Option<&MetricValue>,
    a1: Option<&MetricValue>,
    b1: Option<&MetricValue>,
) -> Comparison {
    let (Some(a0), Some(b0), Some(a1), Some(b1)) = (a0, b0, a1, b1) else {
        return Comparison::Unavailable(Unavailable::Missing);
    };
    let all = [a0, b0, a1, b1];
    if all
        .iter()
        .any(|m| m.identity.kind != MetricKind::Evaluation)
    {
        return Comparison::Unavailable(Unavailable::NotAnEvaluation);
    }
    if all.iter().any(|m| m.identity != a0.identity) {
        return Comparison::Unavailable(Unavailable::IncompatibleIdentity);
    }
    Comparison::Available((b1.value - a1.value) - (b0.value - a0.value))
}
