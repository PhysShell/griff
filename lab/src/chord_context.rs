//! Exact, Lab-only preceding-context views over chord assignments.
//!
//! The observed target string is used only after solving, as a label whose
//! rank is measured. It never enters an assignment score or hard constraint.

use griff_core::event::{FretboardPosition, Tuning};

use crate::chord::{
    analyze_chord, ChordAnalysis, ChordAtom, ChordCostPolicy, ChordError, TargetStringConstraint,
};

/// Imported preceding context registered for one chord target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ChordContext {
    /// Fret of the imported legato origin.
    pub origin_fret: u8,
    /// Latest preceding fretting-hand anchor; absence remains explicit.
    pub anchor_fret: Option<u8>,
}

/// Context values for one complete chord assignment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct AssignmentContext {
    /// Absolute target-candidate fret distance from the legato origin.
    pub origin_distance: i64,
    /// Sum of distances of fretted atoms from the anchor. Open atoms add zero.
    pub anchor_distance: Option<i64>,
}

/// Exact scalar minimum under one target-string condition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExactMinimum {
    /// Minimum value.
    pub optimum: i64,
    /// Number of assignments attaining it (saturating).
    pub optimum_count: u64,
    /// First minimum in deterministic candidate order.
    pub chosen: Vec<FretboardPosition>,
}

/// Exact lexicographic minimum under one target-string condition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExactLexMinimum {
    /// First component.
    pub first: i64,
    /// Second component.
    pub second: i64,
    /// Number of assignments attaining the pair (saturating).
    pub optimum_count: u64,
    /// First minimum in deterministic candidate order.
    pub chosen: Vec<FretboardPosition>,
}

/// All registered exact views for one legal target string.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextStringResult {
    /// Legal target string.
    pub string: u8,
    /// Frozen B0 minimum; absent only when this legal condition is infeasible.
    pub base: Option<ExactMinimum>,
    /// Origin-distance minimum.
    pub origin: Option<ExactMinimum>,
    /// Anchor-distance minimum; absent when condition or anchor is absent.
    pub anchor: Option<ExactMinimum>,
    /// Exact `(O, A)` lexicographic minimum.
    pub origin_anchor: Option<ExactLexMinimum>,
    /// Exact `(A, O)` lexicographic minimum.
    pub anchor_origin: Option<ExactLexMinimum>,
    /// Whether any assignment for this condition is on the global frontier.
    pub pareto_member: Option<bool>,
    /// Number of nondominated assignments for this condition (saturating).
    pub pareto_count: Option<u64>,
    /// First nondominated assignment for this condition.
    pub pareto_chosen: Option<Vec<FretboardPosition>>,
}

/// Direction of a rank change relative to B0.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ContextClassification {
    /// Context rank is smaller (better).
    Improved,
    /// Context and B0 dense ranks are equal.
    Unchanged,
    /// Context rank is larger (worse).
    Worsened,
    /// One rank is unavailable.
    Unavailable,
}

/// One observed-label rank comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct RankChange {
    /// `B0 rank - context rank`; positive means improvement.
    pub rank_delta: Option<i64>,
    /// Sign classification.
    pub classification: ContextClassification,
}

/// Observed target-string ranks under every registered view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservedContext {
    /// Observed condition's frozen B0 optimum.
    pub base_optimum: Option<i64>,
    /// Frozen B0 dense rank.
    pub base_rank: Option<usize>,
    /// Origin-distance dense rank.
    pub origin_rank: Option<usize>,
    /// Anchor-distance dense rank.
    pub anchor_rank: Option<usize>,
    /// `(O, A)` dense rank.
    pub origin_anchor_rank: Option<usize>,
    /// `(A, O)` dense rank.
    pub anchor_origin_rank: Option<usize>,
    /// Global Pareto membership of the observed condition.
    pub pareto_member: Option<bool>,
    /// O vs B0.
    pub origin_change: RankChange,
    /// A vs B0.
    pub anchor_change: RankChange,
    /// O→A vs B0.
    pub origin_anchor_change: RankChange,
    /// A→O vs B0.
    pub anchor_origin_change: RankChange,
}

/// Descriptive imported realization under the registered context views.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HumanContextAssessment {
    /// Imported positions in stable atom order.
    pub positions: Vec<FretboardPosition>,
    /// B0 value, when assignment-feasible.
    pub base: Option<i64>,
    /// O value, when assignment-feasible.
    pub origin: Option<i64>,
    /// A value, when assignment-feasible and an anchor exists.
    pub anchor: Option<i64>,
    /// O excess over the observed condition's exact O minimum.
    pub origin_excess: Option<i64>,
    /// A excess over the observed condition's exact A minimum.
    pub anchor_excess: Option<i64>,
    /// Whether the imported assignment itself is globally nondominated.
    pub pareto_member: Option<bool>,
}

/// Frozen #209 analysis plus every context view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChordContextAnalysis {
    /// Unchanged #209 result.
    pub baseline: ChordAnalysis,
    /// Full legal target-string domain.
    pub target_strings: Vec<ContextStringResult>,
    /// Observed-label ranks and changes.
    pub observed: ObservedContext,
    /// Imported realization, when complete.
    pub human: Option<HumanContextAssessment>,
}

#[derive(Clone)]
struct Assignment {
    string: u8,
    positions: Vec<FretboardPosition>,
    base: i64,
    origin: i64,
    anchor: Option<i64>,
}

/// Scores one assignment using only preceding fret context.
#[must_use]
pub fn assignment_context(
    positions: &[FretboardPosition],
    target_index: usize,
    context: ChordContext,
) -> Option<AssignmentContext> {
    let target = positions.get(target_index)?;
    let origin_distance = i64::from(target.fret.abs_diff(context.origin_fret));
    let anchor_distance = context.anchor_fret.map(|anchor| {
        positions
            .iter()
            .filter(|position| position.fret > 0)
            .map(|position| i64::from(position.fret.abs_diff(anchor)))
            .fold(0_i64, i64::saturating_add)
    });
    Some(AssignmentContext {
        origin_distance,
        anchor_distance,
    })
}

/// Enumerates U once, then derives exact B0/O/A/lexicographic/Pareto views.
///
/// # Errors
///
/// Preserves the #209 oracle's typed malformed-input and infeasible-U errors.
#[allow(clippy::too_many_arguments)] // transparent registered experiment inputs
pub fn analyze_chord_context(
    atoms: &[ChordAtom],
    tuning: &Tuning,
    max_fret: u8,
    target_atom_id: usize,
    observed_string: u8,
    context: ChordContext,
    policy: &ChordCostPolicy,
) -> Result<ChordContextAnalysis, ChordError> {
    let baseline = analyze_chord(
        atoms,
        tuning,
        max_fret,
        TargetStringConstraint {
            atom_id: target_atom_id,
            string: observed_string,
        },
        policy,
    )?;
    let target_index = atoms
        .iter()
        .position(|atom| atom.note_id == target_atom_id)
        .ok_or(ChordError::MissingTarget(target_atom_id))?;
    let candidates: Vec<Vec<FretboardPosition>> = atoms
        .iter()
        .map(|atom| tuning.candidates(atom.pitch, max_fret))
        .collect();
    let mut assignments = Vec::new();
    enumerate(
        &candidates,
        target_index,
        context,
        *policy,
        0,
        &mut [false; 256],
        &mut Vec::with_capacity(atoms.len()),
        0,
        &mut assignments,
    );
    let frontier: Vec<bool> = if context.anchor_fret.is_some() {
        (0..assignments.len())
            .map(|index| !is_dominated(index, &assignments))
            .collect()
    } else {
        Vec::new()
    };

    let mut target_strings = Vec::with_capacity(baseline.target_strings.len());
    for baseline_condition in &baseline.target_strings {
        let members: Vec<(usize, &Assignment)> = assignments
            .iter()
            .enumerate()
            .filter(|(_, assignment)| assignment.string == baseline_condition.string)
            .collect();
        let base = scalar_minimum(&members, |assignment| assignment.base);
        let origin = scalar_minimum(&members, |assignment| assignment.origin);
        let anchor = context
            .anchor_fret
            .and_then(|_| scalar_minimum(&members, |assignment| assignment.anchor.unwrap_or(0)));
        let origin_anchor = context.anchor_fret.and_then(|_| {
            lex_minimum(&members, |assignment| {
                (assignment.origin, assignment.anchor.unwrap_or(0))
            })
        });
        let anchor_origin = context.anchor_fret.and_then(|_| {
            lex_minimum(&members, |assignment| {
                (assignment.anchor.unwrap_or(0), assignment.origin)
            })
        });
        let pareto_indices: Vec<usize> = members
            .iter()
            .map(|(index, _)| *index)
            .filter(|index| frontier.get(*index).copied().unwrap_or(false))
            .collect();
        let (pareto_member, pareto_count, pareto_chosen) = if context.anchor_fret.is_some() {
            (
                Some(!pareto_indices.is_empty()),
                Some(u64::try_from(pareto_indices.len()).unwrap_or(u64::MAX)),
                pareto_indices
                    .first()
                    .map(|index| assignments[*index].positions.clone()),
            )
        } else {
            (None, None, None)
        };
        target_strings.push(ContextStringResult {
            string: baseline_condition.string,
            base,
            origin,
            anchor,
            origin_anchor,
            anchor_origin,
            pareto_member,
            pareto_count,
            pareto_chosen,
        });
    }

    let base_rank = rank_scalar(&target_strings, observed_string, |condition| {
        condition.base.as_ref().map(|metric| metric.optimum)
    });
    let origin_rank = rank_scalar(&target_strings, observed_string, |condition| {
        condition.origin.as_ref().map(|metric| metric.optimum)
    });
    let anchor_rank = rank_scalar(&target_strings, observed_string, |condition| {
        condition.anchor.as_ref().map(|metric| metric.optimum)
    });
    let origin_anchor_rank = rank_pair(&target_strings, observed_string, |condition| {
        condition
            .origin_anchor
            .as_ref()
            .map(|metric| (metric.first, metric.second))
    });
    let anchor_origin_rank = rank_pair(&target_strings, observed_string, |condition| {
        condition
            .anchor_origin
            .as_ref()
            .map(|metric| (metric.first, metric.second))
    });
    let observed_condition = target_strings
        .iter()
        .find(|condition| condition.string == observed_string);
    let observed = ObservedContext {
        base_optimum: observed_condition
            .and_then(|condition| condition.base.as_ref())
            .map(|metric| metric.optimum),
        base_rank,
        origin_rank,
        anchor_rank,
        origin_anchor_rank,
        anchor_origin_rank,
        pareto_member: observed_condition.and_then(|condition| condition.pareto_member),
        origin_change: rank_change(base_rank, origin_rank),
        anchor_change: rank_change(base_rank, anchor_rank),
        origin_anchor_change: rank_change(base_rank, origin_anchor_rank),
        anchor_origin_change: rank_change(base_rank, anchor_origin_rank),
    };
    let human = human_assessment(
        atoms,
        target_index,
        observed_condition,
        context,
        policy,
        &assignments,
        &frontier,
    );
    Ok(ChordContextAnalysis {
        baseline,
        target_strings,
        observed,
        human,
    })
}

#[allow(clippy::too_many_arguments)]
fn enumerate(
    candidates: &[Vec<FretboardPosition>],
    target_index: usize,
    context: ChordContext,
    policy: ChordCostPolicy,
    atom: usize,
    used: &mut [bool; 256],
    path: &mut Vec<FretboardPosition>,
    base: i64,
    out: &mut Vec<Assignment>,
) {
    if atom == candidates.len() {
        if let Some(values) = assignment_context(path, target_index, context) {
            out.push(Assignment {
                string: path[target_index].string,
                positions: path.clone(),
                base,
                origin: values.origin_distance,
                anchor: values.anchor_distance,
            });
        }
        return;
    }
    for &candidate in &candidates[atom] {
        let string = usize::from(candidate.string);
        if used[string] {
            continue;
        }
        used[string] = true;
        path.push(candidate);
        enumerate(
            candidates,
            target_index,
            context,
            policy,
            atom.saturating_add(1),
            used,
            path,
            base.saturating_add(policy.position_cost(candidate)),
            out,
        );
        path.pop();
        used[string] = false;
    }
}

fn scalar_minimum(
    members: &[(usize, &Assignment)],
    value: impl Fn(&Assignment) -> i64,
) -> Option<ExactMinimum> {
    let first = members.first()?.1;
    let mut optimum = value(first);
    let mut count = 0_u64;
    let mut chosen = first.positions.clone();
    for (_, assignment) in members {
        let candidate = value(assignment);
        if candidate < optimum {
            optimum = candidate;
            count = 1;
            chosen.clone_from(&assignment.positions);
        } else if candidate == optimum {
            count = count.saturating_add(1);
        }
    }
    Some(ExactMinimum {
        optimum,
        optimum_count: count,
        chosen,
    })
}

fn lex_minimum(
    members: &[(usize, &Assignment)],
    value: impl Fn(&Assignment) -> (i64, i64),
) -> Option<ExactLexMinimum> {
    let first = members.first()?.1;
    let mut optimum = value(first);
    let mut count = 0_u64;
    let mut chosen = first.positions.clone();
    for (_, assignment) in members {
        let candidate = value(assignment);
        if candidate < optimum {
            optimum = candidate;
            count = 1;
            chosen.clone_from(&assignment.positions);
        } else if candidate == optimum {
            count = count.saturating_add(1);
        }
    }
    Some(ExactLexMinimum {
        first: optimum.0,
        second: optimum.1,
        optimum_count: count,
        chosen,
    })
}

fn is_dominated(index: usize, assignments: &[Assignment]) -> bool {
    let candidate = &assignments[index];
    assignments.iter().enumerate().any(|(other_index, other)| {
        if other_index == index {
            return false;
        }
        let (Some(candidate_anchor), Some(other_anchor)) = (candidate.anchor, other.anchor) else {
            return false;
        };
        other.base <= candidate.base
            && other.origin <= candidate.origin
            && other_anchor <= candidate_anchor
            && (other.base < candidate.base
                || other.origin < candidate.origin
                || other_anchor < candidate_anchor)
    })
}

fn rank_scalar(
    conditions: &[ContextStringResult],
    observed_string: u8,
    value: impl Fn(&ContextStringResult) -> Option<i64>,
) -> Option<usize> {
    let observed = conditions
        .iter()
        .find(|condition| condition.string == observed_string)
        .and_then(&value)?;
    let mut cheaper: Vec<i64> = conditions
        .iter()
        .filter_map(value)
        .filter(|candidate| *candidate < observed)
        .collect();
    cheaper.sort_unstable();
    cheaper.dedup();
    Some(cheaper.len().saturating_add(1))
}

fn rank_pair(
    conditions: &[ContextStringResult],
    observed_string: u8,
    value: impl Fn(&ContextStringResult) -> Option<(i64, i64)>,
) -> Option<usize> {
    let observed = conditions
        .iter()
        .find(|condition| condition.string == observed_string)
        .and_then(&value)?;
    let mut cheaper: Vec<(i64, i64)> = conditions
        .iter()
        .filter_map(value)
        .filter(|candidate| *candidate < observed)
        .collect();
    cheaper.sort_unstable();
    cheaper.dedup();
    Some(cheaper.len().saturating_add(1))
}

fn rank_change(base: Option<usize>, context: Option<usize>) -> RankChange {
    let rank_delta = base.zip(context).map(|(base, context)| {
        i64::try_from(base).unwrap_or(i64::MAX) - i64::try_from(context).unwrap_or(i64::MAX)
    });
    let classification = match rank_delta {
        Some(delta) if delta > 0 => ContextClassification::Improved,
        Some(0) => ContextClassification::Unchanged,
        Some(_) => ContextClassification::Worsened,
        None => ContextClassification::Unavailable,
    };
    RankChange {
        rank_delta,
        classification,
    }
}

#[allow(clippy::too_many_arguments)]
fn human_assessment(
    atoms: &[ChordAtom],
    target_index: usize,
    observed: Option<&ContextStringResult>,
    context: ChordContext,
    policy: &ChordCostPolicy,
    assignments: &[Assignment],
    frontier: &[bool],
) -> Option<HumanContextAssessment> {
    let positions: Vec<FretboardPosition> = atoms
        .iter()
        .map(|atom| atom.imported_position)
        .collect::<Option<_>>()?;
    let matching = assignments
        .iter()
        .position(|assignment| assignment.positions == positions);
    let values = assignment_context(&positions, target_index, context)?;
    let base = matching.map(|_| {
        positions
            .iter()
            .map(|position| policy.position_cost(*position))
            .fold(0_i64, i64::saturating_add)
    });
    let origin = matching.map(|_| values.origin_distance);
    let anchor = matching.and(values.anchor_distance);
    Some(HumanContextAssessment {
        positions,
        base,
        origin,
        anchor,
        origin_excess: origin
            .zip(
                observed
                    .and_then(|condition| condition.origin.as_ref())
                    .map(|metric| metric.optimum),
            )
            .map(|(value, optimum)| value.saturating_sub(optimum)),
        anchor_excess: anchor
            .zip(
                observed
                    .and_then(|condition| condition.anchor.as_ref())
                    .map(|metric| metric.optimum),
            )
            .map(|(value, optimum)| value.saturating_sub(optimum)),
        pareto_member: context.anchor_fret.map(|_| {
            matching
                .and_then(|index| frontier.get(index))
                .copied()
                .unwrap_or(false)
        }),
    })
}
