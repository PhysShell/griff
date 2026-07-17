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
#[derive(Debug, Clone, PartialEq)]
pub enum PathError {
    /// The problem had no layers at all.
    NoLayers,
    /// Layer `layer` had no states, so no path can cross it.
    EmptyLayer {
        /// The empty layer's index.
        layer: usize,
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
pub fn solve(problem: &LayeredProblem) -> Result<PathSolution, PathError> {
    let _ = problem;
    unimplemented!("layered_path::solve")
}

#[cfg(test)]
#[allow(
    clippy::missing_assert_message,
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::indexing_slicing
)]
mod tests {
    use super::{solve, EdgeId, LayeredProblem, PathError, StateId};
    use crate::scoring::{Axes, Axis, WeightPolicy};

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
        // Two paths cost exactly 2.0: [0,1] and [1,0]. Lexicographic order picks
        // [0,1] — the earliest layer decides.
        let locals = locals_of(&[&[0.0, 1.0], &[1.0, 0.0]]);
        let transitions = transitions_of(&[&[&[9.0, 1.0], &[1.0, 9.0]]]);
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
        let summed: f64 = solution.steps.iter().map(Scored_aggregate).sum::<f64>()
            + solution.edges.iter().map(Scored_aggregate).sum::<f64>();
        assert!(
            (summed - solution.total_cost).abs() < 1e-12,
            "the trace explains the whole total",
        );
    }

    /// Sums a `Scored`'s rationale — a free function so the two map calls above
    /// stay readable.
    #[allow(non_snake_case)]
    fn Scored_aggregate<T>(s: &crate::scoring::Scored<T>) -> f64 {
        s.aggregate()
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
        let _ = solve(&LayeredProblem {
            locals: &locals,
            transitions: &transitions,
            policy: &p,
        })
        .expect("solves");
        let after: Vec<Vec<f64>> = locals
            .iter()
            .map(|l| l.iter().map(|a| a.get("local").unwrap()).collect())
            .collect();
        assert_eq!(before, after, "the engine borrows; it never mutates");
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

                // Brute force: enumerate every path, keep (cost, path) minimal
                // with lexicographic tie-breaking — three nested loops, no crate.
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
                    // Odometer over the shape.
                    let mut i = layers;
                    loop {
                        if i == 0 {
                            break;
                        }
                        i -= 1;
                        path[i] += 1;
                        if path[i] < shape[i] {
                            break;
                        }
                        path[i] = 0;
                        if i == 0 {
                            // wrapped fully
                            i = usize::MAX;
                            break;
                        }
                    }
                    if i == usize::MAX {
                        break;
                    }
                }
                let (want_cost, want_path) = best.expect("at least one path");
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
