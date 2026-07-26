//! Exact reference solver for the spike IR.
//!
//! Deterministic backtracking: variables in declaration order, values
//! ascending. Unary interval-class constraints are pre-filtered into domains;
//! the band constraint uses two *sound* partial prunes (a band only grows as
//! assignments accumulate) plus an exact leaf check that mirrors production's
//! `band_overlap` integer arithmetic. Adequate for spike-sized problems; a
//! real Lab needs propagation, and saying so is part of the spike's output.

use crate::ir::{Constraint, IntVar, OracleProblem};

/// Reference-solver outcome.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// A satisfying assignment (one value per variable, declaration order).
    Sat {
        /// The witness assignment.
        witness: Vec<i64>,
        /// Assignments explored.
        nodes: u64,
    },
    /// The whole search space is exhausted without a witness.
    Unsat {
        /// Assignments explored.
        nodes: u64,
    },
}

/// Solves the problem exactly by deterministic backtracking.
#[must_use]
pub fn solve_exact(problem: &OracleProblem) -> Outcome {
    let domains = prefiltered_domains(problem);
    if domains.iter().any(Vec::is_empty) {
        return Outcome::Unsat { nodes: 0 };
    }

    let mut assignment: Vec<i64> = Vec::with_capacity(problem.vars().len());
    let mut nodes: u64 = 0;
    if search(problem, &domains, &mut assignment, &mut nodes) {
        Outcome::Sat {
            witness: assignment,
            nodes,
        }
    } else {
        Outcome::Unsat { nodes }
    }
}

/// Propagating solver — the same outcome contract as [`solve_exact`], but with
/// forward-checking propagation of the binary travel bound layered onto the
/// search, so branches that plain backtracking would enter are pruned early.
///
/// Only *sound* pruning is applied: a value is dropped from a variable's domain
/// only when it violates an [`Constraint::AbsDiffLe`] against an already-assigned
/// partner, and a branch is abandoned only when some still-unassigned variable
/// has no travel-consistent value left. The whole-band [`Constraint::BandOverlapAtMost`]
/// keeps the reference's exact partial prune and leaf check. Because every prune
/// removes only values/branches that provably extend to no solution, the solver
/// reports `Sat` iff a solution exists — i.e. it agrees with [`solve_exact`] on
/// every problem — and it is deterministic (variables in declaration order,
/// values ascending, as the reference).
#[must_use]
pub fn solve_propagate(problem: &OracleProblem) -> Outcome {
    let base = prefiltered_domains(problem);
    if base.iter().any(Vec::is_empty) {
        return Outcome::Unsat { nodes: 0 };
    }
    let mut assignment: Vec<i64> = Vec::with_capacity(problem.vars().len());
    let mut nodes: u64 = 0;
    if mac_search(problem, &base, &mut assignment, &mut nodes) {
        Outcome::Sat {
            witness: assignment,
            nodes,
        }
    } else {
        Outcome::Unsat { nodes }
    }
}

/// Backtracking search with forward checking of the travel bound.
fn mac_search(
    problem: &OracleProblem,
    base: &[Vec<i64>],
    assignment: &mut Vec<i64>,
    nodes: &mut u64,
) -> bool {
    if assignment.len() == problem.vars().len() {
        return problem
            .constraints()
            .iter()
            .all(|c| check_full(c, assignment));
    }
    let index = assignment.len();
    let Some(domain) = base.get(index) else {
        return false;
    };
    for &value in domain {
        // Skip values that break the travel bound against an assigned partner —
        // a sound prune that never removes a solution value.
        if !absdiff_ok_for(problem, assignment, index, value) {
            continue;
        }
        *nodes = nodes.saturating_add(1);
        assignment.push(value);
        if consistent_partial(problem, assignment)
            && forward_check_ok(problem, base, assignment)
            && mac_search(problem, base, assignment, nodes)
        {
            return true;
        }
        assignment.pop();
    }
    false
}

/// Whether assigning variable `index` the value `value` respects every
/// [`Constraint::AbsDiffLe`] whose *other* endpoint is already assigned.
fn absdiff_ok_for(problem: &OracleProblem, assignment: &[i64], index: usize, value: i64) -> bool {
    problem.constraints().iter().all(|constraint| {
        let Constraint::AbsDiffLe { a, b, bound } = constraint else {
            return true;
        };
        let partner = if a.0 == index {
            b.0
        } else if b.0 == index {
            a.0
        } else {
            return true;
        };
        assignment
            .get(partner)
            .is_none_or(|&other| (value - other).abs() <= *bound)
    })
}

/// Forward check: every still-unassigned variable retains at least one value
/// consistent with the travel bound against the current prefix. A wipe-out here
/// means the prefix extends to no solution, so the branch is pruned — soundly,
/// since only travel-inconsistent values are excluded from the count.
fn forward_check_ok(problem: &OracleProblem, base: &[Vec<i64>], assignment: &[i64]) -> bool {
    (assignment.len()..problem.vars().len()).all(|j| {
        base.get(j).is_some_and(|domain| {
            domain
                .iter()
                .any(|&value| absdiff_ok_for(problem, assignment, j, value))
        })
    })
}

/// Checks a complete assignment against the problem: full length, every
/// value inside its variable's domain, and every constraint satisfied.
/// The check the external-solver cross-check runs on parsed witnesses.
#[must_use]
pub fn verify_witness(problem: &OracleProblem, witness: &[i64]) -> bool {
    witness.len() == problem.vars().len()
        && problem
            .vars()
            .iter()
            .zip(witness)
            .all(|(var, value)| var.domain.contains(value))
        && problem.constraints().iter().all(|c| check_full(c, witness))
}

/// Applies unary constraints (forbidden interval classes) to the domains.
fn prefiltered_domains(problem: &OracleProblem) -> Vec<Vec<i64>> {
    let mut domains: Vec<Vec<i64>> = problem
        .vars()
        .iter()
        .map(|v: &IntVar| v.domain.clone())
        .collect();
    for constraint in problem.constraints() {
        if let Constraint::ForbiddenIntervalClasses {
            var,
            fixed,
            classes,
        } = constraint
        {
            if let Some(domain) = domains.get_mut(var.0) {
                domain.retain(|&value| {
                    let class = (value - fixed).abs().rem_euclid(12);
                    !classes.contains(&class)
                });
            }
        }
    }
    domains
}

fn search(
    problem: &OracleProblem,
    domains: &[Vec<i64>],
    assignment: &mut Vec<i64>,
    nodes: &mut u64,
) -> bool {
    if assignment.len() == problem.vars().len() {
        return problem
            .constraints()
            .iter()
            .all(|c| check_full(c, assignment));
    }
    let index = assignment.len();
    let Some(domain) = domains.get(index) else {
        return false;
    };
    for &value in domain {
        *nodes = nodes.saturating_add(1);
        assignment.push(value);
        if consistent_partial(problem, assignment) && search(problem, domains, assignment, nodes) {
            return true;
        }
        assignment.pop();
    }
    false
}

/// Sound partial checks over the currently assigned prefix.
fn consistent_partial(problem: &OracleProblem, assignment: &[i64]) -> bool {
    problem.constraints().iter().all(|constraint| {
        match constraint {
            Constraint::AbsDiffLe { a, b, bound } => {
                match (assignment.get(a.0), assignment.get(b.0)) {
                    (Some(&x), Some(&y)) => (x - y).abs() <= *bound,
                    _ => true,
                }
            }
            Constraint::ForbiddenIntervalClasses {
                var,
                fixed,
                classes,
            } => assignment.get(var.0).is_none_or(|&value| {
                let class = (value - fixed).abs().rem_euclid(12);
                !classes.contains(&class)
            }),
            Constraint::BandOverlapAtMost {
                vars,
                fixed_lo,
                fixed_hi,
                num,
                den,
            } => {
                let Some((b_lo, b_hi)) = running_band(vars, assignment) else {
                    return true; // no band member assigned yet
                };
                let a_span = fixed_hi - fixed_lo;
                let i_lo = (*fixed_lo).max(b_lo);
                let i_hi = (*fixed_hi).min(b_hi);
                let ov = (i_hi - i_lo).max(0);
                if a_span == 0 {
                    // The band only grows: once the fixed point is inside,
                    // the intersection can never become empty again.
                    return i_hi < i_lo;
                }
                if b_hi - b_lo >= a_span {
                    // narrower is already pinned to the fixed span, and the
                    // overlap can only grow: prune when already too large.
                    return ov * den <= a_span * num;
                }
                true
            }
        }
    })
}

/// Exact whole-assignment check (production `band_overlap` integer form).
fn check_full(constraint: &Constraint, assignment: &[i64]) -> bool {
    match constraint {
        Constraint::AbsDiffLe { a, b, bound } => match (assignment.get(a.0), assignment.get(b.0)) {
            (Some(&x), Some(&y)) => (x - y).abs() <= *bound,
            _ => false,
        },
        Constraint::ForbiddenIntervalClasses {
            var,
            fixed,
            classes,
        } => assignment.get(var.0).is_some_and(|&value| {
            let class = (value - fixed).abs().rem_euclid(12);
            !classes.contains(&class)
        }),
        Constraint::BandOverlapAtMost {
            vars,
            fixed_lo,
            fixed_hi,
            num,
            den,
        } => {
            let Some((b_lo, b_hi)) = running_band(vars, assignment) else {
                return false;
            };
            let i_lo = (*fixed_lo).max(b_lo);
            let i_hi = (*fixed_hi).min(b_hi);
            let ov = (i_hi - i_lo).max(0);
            let narrower = (fixed_hi - fixed_lo).min(b_hi - b_lo);
            if narrower == 0 {
                // Degenerate single-pitch band: clean iff the intersection is
                // empty — mirroring `band_overlap`'s 1.0-iff-inside rule.
                i_hi < i_lo
            } else {
                ov * den <= narrower * num
            }
        }
    }
}

/// Min/max over the assigned members of `vars`, if any.
fn running_band(vars: &[crate::ir::VarId], assignment: &[i64]) -> Option<(i64, i64)> {
    let mut band: Option<(i64, i64)> = None;
    for var in vars {
        if let Some(&value) = assignment.get(var.0) {
            band = Some(match band {
                None => (value, value),
                Some((lo, hi)) => (lo.min(value), hi.max(value)),
            });
        }
    }
    band
}
