//! Deterministic layered-path engine (S7 Slice A, ADR-0013).
//!
//! The *route*, not the map: given ordered layers of feasible states, the
//! caller's local and transition cost facts, and a versioned weight policy,
//! this returns the single best path — one state per layer — by exact dynamic
//! programming.
//!
//! The engine is deliberately domain-free. It knows nothing of notes, bars,
//! strategies, generation, or any frontend; a state is an ordinal in a layer
//! and a cost is a weighted set of caller-supplied [`Axes`]. That is what makes
//! it reusable: the first client (S7 Slice B's multi-bar candidate chain) is a
//! client, not a special case.
//!
//! Determinism (SPEC §6, ADR-0013 §3) comes from construction, not from a seed:
//! exact DP over a fixed cost function has a unique optimum, and exact ties
//! break by the **lexicographically smallest vector of state ordinals**. No RNG
//! and no seed take part in selection.
//!
//! Explainability reuses the ADR-0017 vocabulary from [`crate::scoring`] — the
//! same [`Axes`], [`WeightPolicy`], and [`Scored`] envelope every other score
//! in `griff` wears. The total is *derived* from the retained per-axis
//! rationale, never the only thing kept (the anti-scalar rule, ADR-0017 §2).

use crate::scoring::{Axes, Provenance, Scored, WeightPolicy};

/// A state's address: its layer and its ordinal within that layer.
///
/// The ordinal is the caller's stable order. It is the tie-breaking key, so the
/// caller controls which of two equally-good paths wins by choosing the order
/// it hands the states in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct StateId {
    /// Index of the layer, `0..layers`.
    pub layer: usize,
    /// Index of the state within its layer.
    pub ordinal: usize,
}

/// An edge's address: the two adjacent states it joins.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EdgeId {
    /// The state in layer `i`.
    pub from: StateId,
    /// The state in layer `i + 1`.
    pub to: StateId,
}

/// One layered problem: the caller's cost facts plus the policy weighting them.
///
/// Everything is borrowed — the engine never mutates the layers, and the caller
/// keeps ownership of its facts.
#[derive(Debug, Clone, Copy)]
pub struct LayeredProblem<'a> {
    /// Local cost facts: `locals[i][s]` describes state `s` of layer `i`. The
    /// outer length is the layer count; each inner length is that layer's size.
    pub locals: &'a [Vec<Axes>],
    /// Transition cost facts: `transitions[i][p][s]` describes the edge from
    /// state `p` of layer `i` to state `s` of layer `i + 1`. The outer length
    /// must be `locals.len() - 1`.
    pub transitions: &'a [Vec<Vec<Axes>>],
    /// The versioned policy weighting both local and transition axes. Weights
    /// are data (ADR-0013 §4): an axis the policy does not name contributes
    /// nothing, so the caller must name every axis it wants counted.
    pub policy: &'a WeightPolicy,
}

/// Why a layered problem could not be solved.
///
/// Every variant names *where* the problem is, so a caller can point at the
/// offending layer, state, or edge rather than guess.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PathError {
    /// The problem had no layers at all.
    NoLayers,
    /// Layer `layer` had no states, so no path can cross it.
    EmptyLayer {
        /// The empty layer's index.
        layer: usize,
    },
    /// The number of transition tables is not `layers - 1`.
    ///
    /// A count mismatch is its own fact, not the shape of an imaginary layer:
    /// an extra table joins nothing, and a one-layer problem has nowhere to
    /// put one.
    TransitionCount {
        /// The count the layers require: `layers - 1`.
        expected: usize,
        /// The count the caller supplied.
        found: usize,
    },
    /// The transition table's shape does not match the layers it joins.
    TransitionShape {
        /// The layer the transition table leaves from.
        layer: usize,
        /// The shape the layers require: `(|L[layer]|, |L[layer + 1]|)`.
        expected: (usize, usize),
        /// The shape the caller supplied.
        found: (usize, usize),
    },
    /// A local cost was not finite (`NaN`, `+∞`, or `-∞`), so no total order
    /// over costs exists.
    NonFiniteLocal {
        /// The offending state.
        state: StateId,
        /// The non-finite aggregate.
        cost: f64,
    },
    /// A transition cost was not finite, so no total order over costs exists.
    NonFiniteTransition {
        /// The offending edge.
        edge: EdgeId,
        /// The non-finite aggregate.
        cost: f64,
    },
    /// Finite costs accumulated to a non-finite running total.
    ///
    /// Individually finite aggregates do not make a finite *path*: a sum can
    /// still overflow to `±∞`. The solver refuses rather than clamp, and never
    /// returns a solution whose total is not finite.
    NonFiniteAccumulation {
        /// The state whose completion (or running total) went non-finite.
        state: StateId,
        /// The offending accumulated value.
        cost: f64,
    },
}

/// The deterministic best path: one state per layer, with its explanation.
///
/// `total_cost` is derived from the retained rationales — the trace is the
/// truth, the scalar is a convenience.
#[derive(Debug, Clone)]
pub struct PathSolution {
    /// The selected state of each layer, in layer order, each with its local
    /// axes, rationale, and provenance.
    pub steps: Vec<Scored<StateId>>,
    /// The selected edge between each adjacent pair, in layer order. Length is
    /// `steps.len() - 1`.
    pub edges: Vec<Scored<EdgeId>>,
    /// `Σ selected local costs + Σ selected transition costs`.
    pub total_cost: f64,
    /// The policy the costs were weighed under (no seed: selection uses none).
    pub provenance: Provenance,
}

impl PathSolution {
    /// The selected ordinal of each layer, in layer order — the vector the
    /// tie-breaking rule minimises lexicographically.
    #[must_use]
    pub fn ordinals(&self) -> Vec<usize> {
        self.steps.iter().map(|s| s.value.ordinal).collect()
    }
}

/// Solves a layered problem exactly: the minimum-cost path, ties broken by the
/// lexicographically smallest vector of state ordinals.
///
/// Minimises `Σ local(selected) + Σ transition(selected adjacent pairs)` by
/// dynamic programming over the layered DAG, in
/// `O(Σᵢ |L[i-1]| × |L[i]|)` — polynomial, never greedy and never a beam.
///
/// # Errors
/// [`PathError`] when the problem has no layers, an empty layer, a transition
/// table whose shape does not match its layers, or any non-finite cost.
pub fn solve(problem: &LayeredProblem<'_>) -> Result<PathSolution, PathError> {
    let layers = problem.locals.len();
    if layers == 0 {
        return Err(PathError::NoLayers);
    }
    for (layer, states) in problem.locals.iter().enumerate() {
        if states.is_empty() {
            return Err(PathError::EmptyLayer { layer });
        }
    }
    // The outer count first: an unreachable extra table must never get to
    // report its own contents as the problem.
    let expected = layers.saturating_sub(1);
    if problem.transitions.len() != expected {
        return Err(PathError::TransitionCount {
            expected,
            found: problem.transitions.len(),
        });
    }
    check_transition_shapes(problem)?;

    let local = score_locals(problem)?;
    let transition = score_transitions(problem)?;

    // Backward pass: `suffix[i][s]` is the cheapest completion from state `s`
    // of layer `i` to the end, its own local cost included. Fallible: finite
    // costs can still accumulate past f64's range.
    let suffix = suffix_costs(&local, &transition)?;

    // Forward pass: walk the optimum, taking the lowest ordinal among exact
    // ties at every layer. Deciding front-to-back is what makes the winner the
    // lexicographically smallest optimal path rather than merely *an* optimum.
    let chosen = walk_lexicographic(&transition, &suffix);

    let steps: Vec<Scored<StateId>> = chosen
        .iter()
        .enumerate()
        .filter_map(|(layer, &ordinal)| local.get(layer)?.get(ordinal).map(|c| c.scored.clone()))
        .collect();
    let edges: Vec<Scored<EdgeId>> = chosen
        .windows(2)
        .enumerate()
        .filter_map(|(layer, pair)| {
            let (from, to) = (*pair.first()?, *pair.get(1)?);
            transition
                .get(layer)?
                .get(from)?
                .get(to)
                .map(|c| c.scored.clone())
        })
        .collect();

    // Derived from the retained rationale — the trace is the truth (ADR-0017 §2).
    // Summed in path order and checked at every step: a finite `suffix` does not
    // make this sum finite, because it adds the same terms in a different order.
    let total_cost = trace_total(&steps, &edges)?;

    Ok(PathSolution {
        steps,
        edges,
        total_cost,
        provenance: Provenance {
            policy_id: problem.policy.id,
            policy_version: problem.policy.version,
            seed: None,
        },
    })
}

/// Sums the selected trace in path order, checking every addition.
///
/// The engine derives its total from the retained rationale rather than from
/// `suffix[0]`, and a different summation order can overflow where `suffix`
/// did not — so this is checked in its own right, and the first state whose
/// addition goes non-finite is named.
fn trace_total(steps: &[Scored<StateId>], edges: &[Scored<EdgeId>]) -> Result<f64, PathError> {
    let mut total = 0.0_f64;
    for (index, step) in steps.iter().enumerate() {
        total += step.aggregate();
        if !total.is_finite() {
            return Err(PathError::NonFiniteAccumulation {
                state: step.value,
                cost: total,
            });
        }
        if let Some(edge) = edges.get(index) {
            total += edge.aggregate();
            if !total.is_finite() {
                return Err(PathError::NonFiniteAccumulation {
                    state: edge.value.to,
                    cost: total,
                });
            }
        }
    }
    Ok(total)
}

/// A scored cost with its aggregate kept beside it, so the DP inner loops read
/// a number instead of re-summing a rationale.
#[derive(Debug, Clone)]
struct Cost<T> {
    scored: Scored<T>,
    aggregate: f64,
}

/// Per layer, per state: the weighed local cost.
type LocalCosts = Vec<Vec<Cost<StateId>>>;

/// Per adjacent layer pair, per `(from, to)`: the weighed transition cost.
type TransitionCosts = Vec<Vec<Vec<Cost<EdgeId>>>>;

/// A borrowed view of [`LocalCosts`].
type LocalCostSlice = [Vec<Cost<StateId>>];

/// A borrowed view of [`TransitionCosts`].
type TransitionCostSlice = [Vec<Vec<Cost<EdgeId>>>];

/// Rejects a transition table whose shape does not match the layers it joins.
fn check_transition_shapes(problem: &LayeredProblem<'_>) -> Result<(), PathError> {
    for layer in 0..problem.locals.len().saturating_sub(1) {
        let expected = (
            problem.locals.get(layer).map_or(0, Vec::len),
            problem
                .locals
                .get(layer.saturating_add(1))
                .map_or(0, Vec::len),
        );
        let table = problem.transitions.get(layer);
        let found = table.map_or((0, 0), |t| (t.len(), t.first().map_or(0, Vec::len)));
        let ok = table
            .is_some_and(|t| t.len() == expected.0 && t.iter().all(|row| row.len() == expected.1));
        if !ok {
            return Err(PathError::TransitionShape {
                layer,
                expected,
                found,
            });
        }
    }
    Ok(())
}

/// Weighs every local axis set, rejecting the first non-finite cost.
fn score_locals(problem: &LayeredProblem<'_>) -> Result<LocalCosts, PathError> {
    problem
        .locals
        .iter()
        .enumerate()
        .map(|(layer, states)| {
            states
                .iter()
                .enumerate()
                .map(|(ordinal, axes)| {
                    let state = StateId { layer, ordinal };
                    let scored = Scored::new(state, axes.clone(), problem.policy, None);
                    let aggregate = scored.aggregate();
                    if aggregate.is_finite() {
                        Ok(Cost { scored, aggregate })
                    } else {
                        Err(PathError::NonFiniteLocal {
                            state,
                            cost: aggregate,
                        })
                    }
                })
                .collect()
        })
        .collect()
}

/// Weighs every transition axis set, rejecting the first non-finite cost.
fn score_transitions(problem: &LayeredProblem<'_>) -> Result<TransitionCosts, PathError> {
    problem
        .transitions
        .iter()
        .enumerate()
        .map(|(layer, table)| {
            table
                .iter()
                .enumerate()
                .map(|(from, row)| {
                    row.iter()
                        .enumerate()
                        .map(|(to, axes)| {
                            let edge = EdgeId {
                                from: StateId {
                                    layer,
                                    ordinal: from,
                                },
                                to: StateId {
                                    layer: layer.saturating_add(1),
                                    ordinal: to,
                                },
                            };
                            let scored = Scored::new(edge, axes.clone(), problem.policy, None);
                            let aggregate = scored.aggregate();
                            if aggregate.is_finite() {
                                Ok(Cost { scored, aggregate })
                            } else {
                                Err(PathError::NonFiniteTransition {
                                    edge,
                                    cost: aggregate,
                                })
                            }
                        })
                        .collect()
                })
                .collect()
        })
        .collect()
}

/// The backward DP: `suffix[i][s] = local(i,s) + min_t(trans(i,s,t) + suffix[i+1][t])`,
/// with the last layer's suffix being its local cost alone.
fn suffix_costs(
    local: &LocalCostSlice,
    transition: &TransitionCostSlice,
) -> Result<Vec<Vec<f64>>, PathError> {
    let layers = local.len();
    let mut back: Vec<Vec<f64>> = Vec::with_capacity(layers);
    let mut next: Vec<f64> = local
        .last()
        .map(|states| states.iter().map(|c| c.aggregate).collect())
        .unwrap_or_default();
    back.push(next.clone());

    for layer in (0..layers.saturating_sub(1)).rev() {
        let states = local.get(layer).map_or(&[][..], Vec::as_slice);
        let table = transition.get(layer);
        let mut current: Vec<f64> = Vec::with_capacity(states.len());
        for (from, cost) in states.iter().enumerate() {
            let state = StateId {
                layer,
                ordinal: from,
            };
            // Every `edge + completion` is checked: two finite costs can still
            // sum to ±∞, and an unchecked ∞ would make distinct alternatives
            // compare equal and hand the tie-break a wrong winner.
            let mut best = f64::INFINITY;
            if let Some(row) = table.and_then(|t| t.get(from)) {
                for (edge, &completion) in row.iter().zip(next.iter()) {
                    let reach = edge.aggregate + completion;
                    if !reach.is_finite() {
                        return Err(PathError::NonFiniteAccumulation { state, cost: reach });
                    }
                    if reach < best {
                        best = reach;
                    }
                }
            }
            let total = cost.aggregate + best;
            if !total.is_finite() {
                return Err(PathError::NonFiniteAccumulation { state, cost: total });
            }
            current.push(total);
        }
        next.clone_from(&current);
        back.push(current);
    }
    back.reverse();
    Ok(back)
}

/// The forward walk over the optimum, lowest ordinal first among exact ties.
fn walk_lexicographic(transition: &TransitionCostSlice, suffix: &[Vec<f64>]) -> Vec<usize> {
    let mut chosen: Vec<usize> = Vec::with_capacity(suffix.len());
    let Some(first) = suffix.first() else {
        return chosen;
    };
    chosen.push(argmin_first(first));

    for layer in 0..suffix.len().saturating_sub(1) {
        let from = chosen.last().copied().unwrap_or(0);
        let completions = suffix
            .get(layer.saturating_add(1))
            .map_or(&[][..], Vec::as_slice);
        let combined: Vec<f64> =
            transition
                .get(layer)
                .and_then(|t| t.get(from))
                .map_or_else(Vec::new, |row| {
                    row.iter()
                        .zip(completions.iter())
                        .map(|(edge, &completion)| edge.aggregate + completion)
                        .collect()
                });
        chosen.push(argmin_first(&combined));
    }
    chosen
}

/// The index of the smallest value; the **first** wins an exact tie, which is
/// what makes the path lexicographically smallest.
fn argmin_first(values: &[f64]) -> usize {
    let mut best_index = 0;
    let mut best_value = f64::INFINITY;
    for (index, &value) in values.iter().enumerate() {
        if value < best_value {
            best_value = value;
            best_index = index;
        }
    }
    best_index
}

#[cfg(test)]
#[allow(
    clippy::missing_assert_message,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]
mod tests {
    use super::{solve, EdgeId, LayeredProblem, PathError, PathSolution, StateId};
    use crate::scoring::{Axes, Axis, Scored, WeightPolicy};

    /// The test policy: one local axis and one transition axis, each weighted
    /// `1.0`, so an axis value *is* its cost and the arithmetic stays readable.
    fn policy() -> WeightPolicy {
        WeightPolicy::new("test_path", 1, vec![("local", 1.0), ("trans", 1.0)])
    }

    fn local(value: f64) -> Axes {
        Axes::new(vec![Axis {
            label: "local",
            value,
        }])
    }

    fn trans(value: f64) -> Axes {
        Axes::new(vec![Axis {
            label: "trans",
            value,
        }])
    }

    /// Builds `locals` from per-layer cost values.
    fn locals_of(values: &[&[f64]]) -> Vec<Vec<Axes>> {
        values
            .iter()
            .map(|layer| layer.iter().map(|&v| local(v)).collect())
            .collect()
    }

    /// Builds `transitions` from per-edge cost matrices.
    fn transitions_of(values: &[&[&[f64]]]) -> Vec<Vec<Vec<Axes>>> {
        values
            .iter()
            .map(|matrix| {
                matrix
                    .iter()
                    .map(|row| row.iter().map(|&v| trans(v)).collect())
                    .collect()
            })
            .collect()
    }

    #[test]
    fn a_single_layer_selects_its_lowest_local_cost() {
        let locals = locals_of(&[&[3.0, 1.0, 2.0]]);
        let transitions: Vec<Vec<Vec<Axes>>> = Vec::new();
        let p = policy();
        let solution = solve(&LayeredProblem {
            locals: &locals,
            transitions: &transitions,
            policy: &p,
        })
        .expect("one layer solves");
        assert_eq!(solution.ordinals(), vec![1]);
        assert!((solution.total_cost - 1.0).abs() < 1e-9);
        assert!(solution.edges.is_empty(), "no edges without a second layer");
    }

    #[test]
    fn one_state_per_layer_returns_that_only_path() {
        let locals = locals_of(&[&[5.0], &[7.0]]);
        let transitions = transitions_of(&[&[&[11.0]]]);
        let p = policy();
        let solution = solve(&LayeredProblem {
            locals: &locals,
            transitions: &transitions,
            policy: &p,
        })
        .expect("solves");
        assert_eq!(solution.ordinals(), vec![0, 0]);
        assert!((solution.total_cost - 23.0).abs() < 1e-9, "5 + 7 + 11");
    }

    #[test]
    fn the_global_optimum_beats_the_greedy_path() {
        // Layer 0: state 0 is locally cheapest (0 vs 1), but every edge out of
        // it is ruinous. The global optimum takes the dearer local state.
        //   greedy:  0 -> 0 -> 0 = 0 + 100 + 0 + 100 + 0 = 200
        //   optimum: 1 -> 1 -> 1 = 1 + 0 + 1 + 0 + 1 = 3
        let locals = locals_of(&[&[0.0, 1.0], &[0.0, 1.0], &[0.0, 1.0]]);
        let transitions = transitions_of(&[
            &[&[100.0, 100.0], &[100.0, 0.0]],
            &[&[100.0, 100.0], &[100.0, 0.0]],
        ]);
        let p = policy();
        let solution = solve(&LayeredProblem {
            locals: &locals,
            transitions: &transitions,
            policy: &p,
        })
        .expect("solves");
        assert_eq!(
            solution.ordinals(),
            vec![1, 1, 1],
            "DP takes the globally best path, not the locally best state",
        );
        assert!((solution.total_cost - 3.0).abs() < 1e-9);
    }

    #[test]
    fn all_equal_costs_select_ordinal_zero_everywhere() {
        let locals = locals_of(&[&[1.0, 1.0, 1.0], &[1.0, 1.0, 1.0]]);
        let transitions =
            transitions_of(&[&[&[1.0, 1.0, 1.0], &[1.0, 1.0, 1.0], &[1.0, 1.0, 1.0]]]);
        let p = policy();
        let solution = solve(&LayeredProblem {
            locals: &locals,
            transitions: &transitions,
            policy: &p,
        })
        .expect("solves");
        assert_eq!(solution.ordinals(), vec![0, 0], "ties break to ordinal 0");
    }

    #[test]
    fn equal_totals_select_the_lexicographically_lowest_path() {
        // Exactly two paths cost 2.0 — [0,1] = 0+2+0 and [1,0] = 1+0+1 — while
        // [0,0] and [1,1] cost 10. Lexicographic order picks [0,1]: the earliest
        // layer decides.
        let locals = locals_of(&[&[0.0, 1.0], &[1.0, 0.0]]);
        let transitions = transitions_of(&[&[&[9.0, 2.0], &[0.0, 9.0]]]);
        let p = policy();
        let solution = solve(&LayeredProblem {
            locals: &locals,
            transitions: &transitions,
            policy: &p,
        })
        .expect("solves");
        assert_eq!(solution.ordinals(), vec![0, 1]);
        assert!((solution.total_cost - 2.0).abs() < 1e-9);
    }

    #[test]
    fn an_empty_problem_reports_no_layers() {
        let locals: Vec<Vec<Axes>> = Vec::new();
        let transitions: Vec<Vec<Vec<Axes>>> = Vec::new();
        let p = policy();
        assert_eq!(
            solve(&LayeredProblem {
                locals: &locals,
                transitions: &transitions,
                policy: &p,
            })
            .unwrap_err(),
            PathError::NoLayers,
        );
    }

    #[test]
    fn an_empty_middle_layer_reports_its_index() {
        let locals = locals_of(&[&[1.0], &[], &[1.0]]);
        let transitions = transitions_of(&[&[&[]], &[]]);
        let p = policy();
        assert_eq!(
            solve(&LayeredProblem {
                locals: &locals,
                transitions: &transitions,
                policy: &p,
            })
            .unwrap_err(),
            PathError::EmptyLayer { layer: 1 },
        );
    }

    #[test]
    fn a_non_finite_local_cost_is_rejected_with_its_location() {
        let locals = locals_of(&[&[1.0, f64::NAN]]);
        let transitions: Vec<Vec<Vec<Axes>>> = Vec::new();
        let p = policy();
        match solve(&LayeredProblem {
            locals: &locals,
            transitions: &transitions,
            policy: &p,
        }) {
            Err(PathError::NonFiniteLocal { state, cost }) => {
                assert_eq!(
                    state,
                    StateId {
                        layer: 0,
                        ordinal: 1
                    }
                );
                assert!(cost.is_nan());
            }
            other => panic!("expected NonFiniteLocal, got {other:?}"),
        }
    }

    #[test]
    fn a_non_finite_transition_cost_is_rejected_with_its_edge() {
        let locals = locals_of(&[&[1.0], &[1.0, 1.0]]);
        let transitions = transitions_of(&[&[&[0.0, f64::INFINITY]]]);
        let p = policy();
        match solve(&LayeredProblem {
            locals: &locals,
            transitions: &transitions,
            policy: &p,
        }) {
            Err(PathError::NonFiniteTransition { edge, cost }) => {
                assert_eq!(
                    edge,
                    EdgeId {
                        from: StateId {
                            layer: 0,
                            ordinal: 0
                        },
                        to: StateId {
                            layer: 1,
                            ordinal: 1
                        },
                    },
                );
                assert!(cost.is_infinite());
            }
            other => panic!("expected NonFiniteTransition, got {other:?}"),
        }
    }

    #[test]
    fn one_layer_with_a_transition_table_is_rejected() {
        // A single layer has nowhere to put a transition: the table joins nothing.
        let locals = locals_of(&[&[1.0]]);
        let transitions = transitions_of(&[&[&[0.0]]]);
        let p = policy();
        assert_eq!(
            solve(&LayeredProblem {
                locals: &locals,
                transitions: &transitions,
                policy: &p,
            })
            .unwrap_err(),
            PathError::TransitionCount {
                expected: 0,
                found: 1,
            },
        );
    }

    #[test]
    fn an_extra_transition_table_is_rejected() {
        let locals = locals_of(&[&[1.0], &[1.0]]);
        let transitions = transitions_of(&[&[&[0.0]], &[&[0.0]]]); // 2 tables, 1 needed
        let p = policy();
        assert_eq!(
            solve(&LayeredProblem {
                locals: &locals,
                transitions: &transitions,
                policy: &p,
            })
            .unwrap_err(),
            PathError::TransitionCount {
                expected: 1,
                found: 2,
            },
        );
    }

    #[test]
    fn a_missing_transition_table_is_rejected() {
        let locals = locals_of(&[&[1.0], &[1.0]]);
        let transitions: Vec<Vec<Vec<Axes>>> = Vec::new();
        let p = policy();
        assert_eq!(
            solve(&LayeredProblem {
                locals: &locals,
                transitions: &transitions,
                policy: &p,
            })
            .unwrap_err(),
            PathError::TransitionCount {
                expected: 1,
                found: 0,
            },
        );
    }

    #[test]
    fn the_exact_transition_count_is_accepted() {
        let locals = locals_of(&[&[1.0], &[1.0], &[1.0]]);
        let transitions = transitions_of(&[&[&[0.0]], &[&[0.0]]]);
        let p = policy();
        solve(&LayeredProblem {
            locals: &locals,
            transitions: &transitions,
            policy: &p,
        })
        .expect("the exact count solves");
    }

    #[test]
    fn an_extra_table_holding_nan_reports_the_count_not_the_nan() {
        // The count is checked before any fact is scored, so an unreachable
        // table's NaN never gets the chance to masquerade as the real problem.
        let locals = locals_of(&[&[1.0]]);
        let transitions = transitions_of(&[&[&[f64::NAN]]]);
        let p = policy();
        assert_eq!(
            solve(&LayeredProblem {
                locals: &locals,
                transitions: &transitions,
                policy: &p,
            })
            .unwrap_err(),
            PathError::TransitionCount {
                expected: 0,
                found: 1,
            },
        );
    }

    #[test]
    fn a_mismatched_transition_table_reports_the_expected_shape() {
        let locals = locals_of(&[&[1.0, 2.0], &[1.0]]);
        let transitions = transitions_of(&[&[&[0.0]]]); // 1x1, but 2x1 is required
        let p = policy();
        assert_eq!(
            solve(&LayeredProblem {
                locals: &locals,
                transitions: &transitions,
                policy: &p,
            })
            .unwrap_err(),
            PathError::TransitionShape {
                layer: 0,
                expected: (2, 1),
                found: (1, 1),
            },
        );
    }

    #[test]
    fn two_finite_costs_overflowing_to_infinity_are_rejected() {
        // Each aggregate is finite; their sum is not. The path is refused, not
        // clamped, and not silently carried as an infinite total.
        let big = f64::MAX * 0.75;
        let locals = locals_of(&[&[big], &[big]]);
        let transitions = transitions_of(&[&[&[0.0]]]);
        let p = policy();
        match solve(&LayeredProblem {
            locals: &locals,
            transitions: &transitions,
            policy: &p,
        }) {
            Err(PathError::NonFiniteAccumulation { cost, .. }) => {
                assert!(cost.is_infinite(), "the accumulation overflowed");
            }
            other => panic!("expected NonFiniteAccumulation, got {other:?}"),
        }
    }

    #[test]
    fn two_finite_costs_underflowing_to_negative_infinity_are_rejected() {
        let small = f64::MIN * 0.75;
        let locals = locals_of(&[&[small], &[small]]);
        let transitions = transitions_of(&[&[&[0.0]]]);
        let p = policy();
        match solve(&LayeredProblem {
            locals: &locals,
            transitions: &transitions,
            policy: &p,
        }) {
            Err(PathError::NonFiniteAccumulation { cost, .. }) => {
                assert!(
                    cost.is_infinite() && cost.is_sign_negative(),
                    "the accumulation underflowed",
                );
            }
            other => panic!("expected NonFiniteAccumulation, got {other:?}"),
        }
    }

    #[test]
    fn overflow_does_not_collapse_distinct_alternatives_onto_ordinal_zero() {
        // Left unchecked, both candidate completions become +inf, compare equal,
        // and the lexicographic rule hands back ordinal 0 — a wrong answer
        // wearing a plausible face. Refusing is the only honest option here.
        let big = f64::MAX * 0.75;
        let locals = locals_of(&[&[0.0, 0.0], &[big, big]]);
        let transitions = transitions_of(&[&[&[big, big * 0.5], &[big, big]]]);
        let p = policy();
        assert!(
            matches!(
                solve(&LayeredProblem {
                    locals: &locals,
                    transitions: &transitions,
                    policy: &p,
                }),
                Err(PathError::NonFiniteAccumulation { .. })
            ),
            "an overflowed comparison must never silently pick ordinal 0",
        );
    }

    #[test]
    fn an_overflow_in_a_later_suffix_layer_names_its_state() {
        let big = f64::MAX * 0.75;
        let locals = locals_of(&[&[0.0], &[big], &[big]]);
        let transitions = transitions_of(&[&[&[0.0]], &[&[0.0]]]);
        let p = policy();
        match solve(&LayeredProblem {
            locals: &locals,
            transitions: &transitions,
            policy: &p,
        }) {
            Err(PathError::NonFiniteAccumulation { state, .. }) => {
                assert_eq!(
                    state,
                    StateId {
                        layer: 1,
                        ordinal: 0
                    },
                    "the layer whose completion overflowed is named",
                );
            }
            other => panic!("expected NonFiniteAccumulation, got {other:?}"),
        }
    }

    #[test]
    fn a_returned_solution_always_has_a_finite_total() {
        let locals = locals_of(&[&[2.0, 5.0], &[3.0, 1.0], &[0.5, 4.0]]);
        let transitions =
            transitions_of(&[&[&[4.0, 0.5], &[1.0, 1.0]], &[&[2.0, 3.0], &[1.5, 0.25]]]);
        let p = policy();
        let solution = solve(&LayeredProblem {
            locals: &locals,
            transitions: &transitions,
            policy: &p,
        })
        .expect("solves");
        assert!(
            solution.total_cost.is_finite(),
            "the solver never returns a non-finite total",
        );
    }

    #[test]
    fn the_explanations_sum_to_the_reported_total() {
        let locals = locals_of(&[&[2.0, 5.0], &[3.0, 1.0]]);
        let transitions = transitions_of(&[&[&[4.0, 0.5], &[1.0, 1.0]]]);
        let p = policy();
        let solution = solve(&LayeredProblem {
            locals: &locals,
            transitions: &transitions,
            policy: &p,
        })
        .expect("solves");
        // Folded in the recurrence's own association, and compared exactly.
        // Summing the steps and the edges in two separate passes is a third
        // grouping again, and "within 1e-12" is how a real disagreement about
        // what the number means gets waved through as rounding.
        assert_eq!(
            canonical_total(&solution),
            solution.total_cost,
            "the trace explains the whole total, in the order it was optimised",
        );
    }

    /// The path cost of a solution's own trace, folded from the end in the
    /// recurrence's association: `local + (edge + rest)`.
    fn canonical_total(solution: &PathSolution) -> f64 {
        let mut steps = solution.steps.iter().rev();
        let mut total = steps.next().map_or(0.0, Scored::aggregate);
        for (step, edge) in steps.zip(solution.edges.iter().rev()) {
            total = edge.aggregate() + total;
            total = step.aggregate() + total;
        }
        total
    }

    /// The selected state ordinals, layer by layer.
    fn ordinals(solution: &PathSolution) -> Vec<usize> {
        solution.steps.iter().map(|s| s.value.ordinal).collect()
    }

    #[test]
    fn the_two_associations_really_do_disagree_here() {
        // Guards the fixture below rather than the engine: `1e16` and `-1e16`
        // cancel exactly, and the ulp at 1e16 is 2, so a 1.0 added *before* the
        // cancellation is rounded away while the same 1.0 added *after* it
        // survives. If f64 ever stopped behaving this way the laws below would
        // pass while proving nothing.
        assert_eq!(1e16 + (-1e16 + 1.0), 0.0, "the 1.0 is absorbed");
        assert_eq!((1e16 + -1e16) + 1.0, 1.0, "the 1.0 survives");
        assert_eq!(1e16 + (-1e16 + 0.5), 0.0);
        assert_eq!((1e16 + -1e16) + 0.5, 0.5);
    }

    #[test]
    fn the_reported_total_is_the_cost_the_recurrence_minimised() {
        // One layer of one state, then two alternatives. Under the recurrence's
        // association both alternatives cost 0.0 — the tail is absorbed — so
        // the tie-break takes ordinal 0. Under a forward left-to-right sum they
        // cost 1.0 and 0.5, and ordinal 1 would win.
        //
        // Two associations, two different winners and two different totals. The
        // engine optimises the first, so the first is what it must report: a
        // total from an arithmetic the selection did not use describes a path
        // the engine did not choose for a reason it did not have.
        let locals = locals_of(&[&[1e16], &[1.0, 0.5]]);
        let transitions = transitions_of(&[&[&[-1e16, -1e16]]]);
        let p = policy();
        let solution = solve(&LayeredProblem {
            locals: &locals,
            transitions: &transitions,
            policy: &p,
        })
        .expect("solves");

        assert_eq!(
            ordinals(&solution),
            vec![0, 0],
            "the recurrence sees a tie and the tie-break takes the lowest ordinal",
        );
        assert_eq!(
            solution.total_cost, 0.0,
            "reported: the cost the recurrence actually minimised",
        );
        assert_eq!(
            canonical_total(&solution),
            solution.total_cost,
            "and the trace re-folds to the very same number",
        );
    }

    #[test]
    fn the_baseline_of_a_single_path_is_folded_the_same_way() {
        // The same fixture with the alternative removed: no tie, no choice, and
        // still the recurrence's association — `1e16 + (-1e16 + 1.0)` is 0.0,
        // not 1.0. A one-state-per-layer problem is a cost evaluation rather
        // than a search, and it must not quietly use different arithmetic from
        // the search that it is the baseline for.
        let locals = locals_of(&[&[1e16], &[1.0]]);
        let transitions = transitions_of(&[&[&[-1e16]]]);
        let p = policy();
        let solution = solve(&LayeredProblem {
            locals: &locals,
            transitions: &transitions,
            policy: &p,
        })
        .expect("solves");
        assert_eq!(solution.total_cost, 0.0);
    }

    #[test]
    fn repeated_calls_return_an_identical_path_and_trace() {
        let locals = locals_of(&[&[2.0, 2.0, 1.0], &[1.0, 3.0, 1.0], &[5.0, 0.0, 5.0]]);
        let transitions = transitions_of(&[
            &[&[1.0, 2.0, 3.0], &[3.0, 2.0, 1.0], &[2.0, 2.0, 2.0]],
            &[&[1.0, 1.0, 1.0], &[2.0, 0.0, 2.0], &[3.0, 3.0, 3.0]],
        ]);
        let p = policy();
        let problem = LayeredProblem {
            locals: &locals,
            transitions: &transitions,
            policy: &p,
        };
        let a = solve(&problem).expect("solves");
        let b = solve(&problem).expect("solves");
        assert_eq!(a.ordinals(), b.ordinals());
        assert!((a.total_cost - b.total_cost).abs() < 1e-12);
        let trace = |s: &super::PathSolution| -> Vec<(f64, f64)> {
            s.steps
                .iter()
                .flat_map(|x| x.rationale.entries().iter().map(|e| (e.value, e.weight)))
                .collect()
        };
        assert_eq!(trace(&a), trace(&b), "the trace is identical too");
    }

    #[test]
    fn the_input_layers_are_not_mutated() {
        let locals = locals_of(&[&[2.0, 5.0], &[3.0, 1.0]]);
        let transitions = transitions_of(&[&[&[4.0, 0.5], &[1.0, 1.0]]]);
        let before: Vec<Vec<f64>> = locals
            .iter()
            .map(|l| l.iter().map(|a| a.get("local").unwrap()).collect())
            .collect();
        let p = policy();
        drop(
            solve(&LayeredProblem {
                locals: &locals,
                transitions: &transitions,
                policy: &p,
            })
            .expect("solves"),
        );
        let after: Vec<Vec<f64>> = locals
            .iter()
            .map(|l| l.iter().map(|a| a.get("local").unwrap()).collect())
            .collect();
        assert_eq!(before, after, "the engine borrows; it never mutates");
    }

    /// The oracle: enumerate every path, keep the minimal `(cost, path)` with
    /// lexicographic tie-breaking. Three nested loops beat a new dependency.
    fn brute_force(
        shape: &[usize],
        local_values: &[Vec<f64>],
        trans_values: &[Vec<Vec<f64>>],
    ) -> (f64, Vec<usize>) {
        let layers = shape.len();
        let mut best: Option<(f64, Vec<usize>)> = None;
        let mut path = vec![0_usize; layers];
        loop {
            let mut total = 0.0;
            for (i, &s) in path.iter().enumerate() {
                total += local_values[i][s];
                if i > 0 {
                    total += trans_values[i - 1][path[i - 1]][s];
                }
            }
            let better = match &best {
                None => true,
                Some((bc, bp)) => {
                    total < *bc - 1e-12
                        || ((total - *bc).abs() <= 1e-12 && path.as_slice() < bp.as_slice())
                }
            };
            if better {
                best = Some((total, path.clone()));
            }
            // Odometer over the shape; `done` when the most significant digit wraps.
            let mut i = layers;
            let mut done = true;
            while i > 0 {
                i -= 1;
                path[i] += 1;
                if path[i] < shape[i] {
                    done = false;
                    break;
                }
                path[i] = 0;
            }
            if done {
                break;
            }
        }
        best.expect("at least one path")
    }

    #[test]
    fn dp_matches_a_brute_force_oracle_on_many_tiny_problems() {
        // An exhaustive check: every tiny problem shape, deterministic integer
        // costs, DP compared against full enumeration for total AND exact path.
        for layers in 1..=4_usize {
            for width in 1..=3_usize {
                let shape: Vec<usize> = (0..layers).map(|_| width).collect();
                // Deterministic pseudo-costs — no RNG, just a fixed mixer.
                let cost_of = |a: usize, b: usize, c: usize| -> f64 {
                    f64::from(u32::try_from((a * 7 + b * 13 + c * 29) % 11).unwrap())
                };
                let local_values: Vec<Vec<f64>> = (0..layers)
                    .map(|i| (0..width).map(|s| cost_of(i, s, 3)).collect())
                    .collect();
                let trans_values: Vec<Vec<Vec<f64>>> = (0..layers.saturating_sub(1))
                    .map(|i| {
                        (0..width)
                            .map(|p| (0..width).map(|s| cost_of(i, p, s)).collect())
                            .collect()
                    })
                    .collect();

                let locals: Vec<Vec<Axes>> = local_values
                    .iter()
                    .map(|l| l.iter().map(|&v| local(v)).collect())
                    .collect();
                let transitions: Vec<Vec<Vec<Axes>>> = trans_values
                    .iter()
                    .map(|m| {
                        m.iter()
                            .map(|r| r.iter().map(|&v| trans(v)).collect())
                            .collect()
                    })
                    .collect();
                let p = policy();
                let got = solve(&LayeredProblem {
                    locals: &locals,
                    transitions: &transitions,
                    policy: &p,
                })
                .expect("solves");

                let (want_cost, want_path) = brute_force(&shape, &local_values, &trans_values);
                assert_eq!(
                    got.ordinals(),
                    want_path,
                    "layers={layers} width={width}: DP path must match the oracle",
                );
                assert!(
                    (got.total_cost - want_cost).abs() < 1e-9,
                    "layers={layers} width={width}: DP total must match the oracle",
                );
            }
        }
    }
}
