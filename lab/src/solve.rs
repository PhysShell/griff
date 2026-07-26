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

    let mut assignment: Vec<i64> = Vec::with_capacity(problem.vars.len());
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

/// Applies unary constraints (forbidden interval classes) to the domains.
fn prefiltered_domains(problem: &OracleProblem) -> Vec<Vec<i64>> {
    let mut domains: Vec<Vec<i64>> = problem
        .vars
        .iter()
        .map(|v: &IntVar| v.domain.clone())
        .collect();
    for constraint in &problem.constraints {
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
    if assignment.len() == problem.vars.len() {
        return problem
            .constraints
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
    problem.constraints.iter().all(|constraint| {
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
