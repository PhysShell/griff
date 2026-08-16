// TDD red phase: deterministic k-best alternatives over the layered path engine
// (S7 Slice C, ADR-0013 as amended by ADR-0030).
//
// Slice A returns *the* optimum. Slice C returns several ranked global paths, so
// S8 can show alternatives and S9 can learn from which one a human picked. The
// stage names four requirements, and this suite pins each:
//
//   - fixed tie-breaking — equal-cost alternatives are ordered by the same
//     lexicographically-smallest-ordinals rule Slice A already selects under, so
//     the *set* is as reproducible as the single winner was;
//   - complete total/local/transition explanations — every alternative is a full
//     `PathSolution`, carrying its per-state and per-edge `Scored` envelopes, and
//     its `total_cost` is the canonical fold of its own trace
//     (`PATH_COST_ASSOCIATION`), not a search by-product;
//   - an explicit diversity rule so alternatives are not path clones — plain
//     k-best over a trellis returns the winner with one layer nudged, which is
//     not an alternative anyone asked to see;
//   - stable provenance — an alternative is identified by its ordinal vector,
//     which survives re-planning, rather than by its rank, which does not.
//
// The oracle is brute force, as in Slice A: every tiny problem shape is
// enumerated exhaustively, folded under the canonical association, ordered, and
// filtered by the same diversity rule, then compared against the engine on both
// the totals and the exact paths.
//
// References `solve_k_best`, `KBestRequest` and `KBestSolution`, none of which
// exist yet, so this suite fails to compile until the green step.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::missing_assert_message,
    clippy::indexing_slicing,
    clippy::float_cmp,
    clippy::arithmetic_side_effects,
    // The canonical fold keeps the accumulator on the *right* of the `+`.
    // `expected += x` is `expected = expected + x`, the left-associated
    // grouping this suite exists to check the engine does not use.
    clippy::assign_op_pattern
)]

use griff_core::{
    layered_path::{
        solve, solve_k_best, KBestRequest, KBestSolution, LayeredProblem, PathError, PathSolution,
    },
    scoring::{Axes, Axis, WeightPolicy},
};

// ── fixtures, in Slice A's shape ─────────────────────────────────────────────

fn policy() -> WeightPolicy {
    WeightPolicy::new("test_k_best", 1, vec![("local", 1.0), ("trans", 1.0)])
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

fn locals_of(values: &[&[f64]]) -> Vec<Vec<Axes>> {
    values
        .iter()
        .map(|layer| layer.iter().map(|&v| local(v)).collect())
        .collect()
}

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

const fn request(k: usize, min_distance: usize) -> KBestRequest {
    KBestRequest { k, min_distance }
}

fn ordinals_of(solution: &KBestSolution) -> Vec<Vec<usize>> {
    solution.paths.iter().map(PathSolution::ordinals).collect()
}

/// A problem's two cost tables, as the engine takes them.
type Tables = (Vec<Vec<Axes>>, Vec<Vec<Vec<Axes>>>);

/// Four layers of three states whose costs spread deterministically — enough
/// structure that the optimum, its near-clones, and genuinely different routes
/// are all distinct paths.
fn spread_problem() -> Tables {
    let locals = locals_of(&[
        &[1.0, 2.0, 4.0],
        &[3.0, 1.0, 2.0],
        &[2.0, 5.0, 1.0],
        &[1.0, 3.0, 2.0],
    ]);
    let transitions = transitions_of(&[
        &[&[1.0, 2.0, 3.0], &[2.0, 1.0, 4.0], &[3.0, 3.0, 1.0]],
        &[&[2.0, 1.0, 3.0], &[1.0, 4.0, 2.0], &[3.0, 2.0, 1.0]],
        &[&[1.0, 3.0, 2.0], &[4.0, 1.0, 3.0], &[2.0, 2.0, 1.0]],
    ]);
    (locals, transitions)
}

// ── the brute-force oracle ───────────────────────────────────────────────────

/// Every ordinal vector the problem admits, in lexicographic order.
fn all_paths(sizes: &[usize]) -> Vec<Vec<usize>> {
    let mut paths = vec![Vec::new()];
    for &size in sizes {
        let mut next = Vec::new();
        for path in &paths {
            for ordinal in 0..size {
                let mut extended = path.clone();
                extended.push(ordinal);
                next.push(extended);
            }
        }
        paths = next;
    }
    paths
}

/// A path's cost folded under `PATH_COST_ASSOCIATION`:
///
/// ```text
/// total = local(last)
/// total = local(i) + (edge(i) + total)   for each preceding layer, in reverse
/// ```
///
/// Recomputed from the raw values rather than from the engine, and grouped
/// exactly as the engine folds, so the comparison is over identical arithmetic
/// and can be exact.
fn oracle_cost(path: &[usize], locals: &[&[f64]], transitions: &[&[&[f64]]]) -> f64 {
    let last = path.len() - 1;
    let mut total = locals[last][path[last]];
    for i in (0..last).rev() {
        let edge = transitions[i][path[i]][path[i + 1]];
        total = locals[i][path[i]] + (edge + total);
    }
    total
}

/// The number of layers two paths disagree on — the diversity measure.
fn distance(a: &[usize], b: &[usize]) -> usize {
    a.iter().zip(b.iter()).filter(|(x, y)| x != y).count()
}

/// The whole answer, computed the slow obvious way: enumerate, fold, order by
/// (cost, ordinals), then take greedily while honouring the diversity rule.
fn oracle_k_best(
    locals: &[&[f64]],
    transitions: &[&[&[f64]]],
    k: usize,
    min_distance: usize,
) -> Vec<Vec<usize>> {
    let sizes: Vec<usize> = locals.iter().map(|l| l.len()).collect();
    let mut scored: Vec<(f64, Vec<usize>)> = all_paths(&sizes)
        .into_iter()
        .map(|p| (oracle_cost(&p, locals, transitions), p))
        .collect();
    // Cost first, then the lexicographically smallest ordinals — the same order
    // the engine's tie-break imposes.
    scored.sort_by(|a, b| a.0.total_cmp(&b.0).then_with(|| a.1.cmp(&b.1)));

    let mut chosen: Vec<Vec<usize>> = Vec::new();
    for (_, path) in scored {
        if chosen.len() == k {
            break;
        }
        if chosen.iter().all(|c| distance(c, &path) >= min_distance) {
            chosen.push(path);
        }
    }
    chosen
}

// ── the first alternative is Slice A's optimum, unchanged ────────────────────

#[test]
fn the_best_of_k_is_exactly_what_solve_returns() {
    let (locals, transitions) = spread_problem();
    let p = policy();
    let problem = LayeredProblem {
        locals: &locals,
        transitions: &transitions,
        policy: &p,
    };

    let single = solve(&problem).expect("solves");
    let many = solve_k_best(&problem, request(4, 1)).expect("solves");

    let first = &many.paths[0];
    assert_eq!(
        first.ordinals(),
        single.ordinals(),
        "k-best does not re-decide the optimum"
    );
    assert_eq!(
        first.total_cost.to_bits(),
        single.total_cost.to_bits(),
        "and reports it to the last bit"
    );
}

// ── the set: ordered, distinct, exactly the oracle's ─────────────────────────

#[test]
fn alternatives_come_back_in_nondecreasing_cost_order() {
    let (locals, transitions) = spread_problem();
    let p = policy();
    let solution = solve_k_best(
        &LayeredProblem {
            locals: &locals,
            transitions: &transitions,
            policy: &p,
        },
        request(6, 1),
    )
    .expect("solves");

    for pair in solution.paths.windows(2) {
        assert!(
            pair[0].total_cost <= pair[1].total_cost,
            "ranked best-first: {} then {}",
            pair[0].total_cost,
            pair[1].total_cost
        );
    }
}

#[test]
fn alternatives_are_distinct_paths() {
    let (locals, transitions) = spread_problem();
    let p = policy();
    let solution = solve_k_best(
        &LayeredProblem {
            locals: &locals,
            transitions: &transitions,
            policy: &p,
        },
        request(8, 1),
    )
    .expect("solves");

    let mut seen = ordinals_of(&solution);
    let before = seen.len();
    seen.sort();
    seen.dedup();
    assert_eq!(seen.len(), before, "no alternative is returned twice");
}

/// Every width vector of `layers` entries, each width in `1..=max_width`.
///
/// The layers of a `LayeredProblem` are deliberately allowed to differ in
/// width, so a sweep that only ever builds rectangular problems is not the
/// sweep its name claims. 1–4 layers over widths 1–3 is 120 vectors; the space
/// is tiny, so it is enumerated rather than sampled.
fn width_vectors(layers: usize, max_width: usize) -> Vec<Vec<usize>> {
    let mut vectors = vec![Vec::new()];
    for _ in 0..layers {
        let mut next = Vec::new();
        for vector in &vectors {
            for width in 1..=max_width {
                let mut extended = vector.clone();
                extended.push(width);
                next.push(extended);
            }
        }
        vectors = next;
    }
    vectors
}

#[test]
fn the_engine_agrees_with_brute_force_on_every_shape_ragged_ones_included() {
    // Slice A's oracle discipline, extended to the whole set *and* to every
    // width vector — not just the square ones. For each shape and each
    // diversity rule the engine's alternatives must be exactly the ones an
    // exhaustive enumeration would pick, in the same order.
    const LOCALS: [&[f64]; 4] = [
        &[1.0, 2.0, 4.0],
        &[3.0, 1.0, 2.0],
        &[2.0, 5.0, 1.0],
        &[1.0, 3.0, 2.0],
    ];
    const TRANSITIONS: [&[&[f64]]; 3] = [
        &[&[1.0, 2.0, 3.0], &[2.0, 1.0, 4.0], &[3.0, 3.0, 1.0]],
        &[&[2.0, 1.0, 3.0], &[1.0, 4.0, 2.0], &[3.0, 2.0, 1.0]],
        &[&[1.0, 3.0, 2.0], &[4.0, 1.0, 3.0], &[2.0, 2.0, 1.0]],
    ];

    let mut shapes_checked = 0_usize;
    let mut ragged_checked = 0_usize;
    for layers in 1..=4_usize {
        for widths in width_vectors(layers, 3) {
            let locals_raw: Vec<&[f64]> =
                (0..layers).map(|l| &LOCALS[l][..widths[l]]).collect();
            // Row count is this layer's width, column count the next layer's.
            let owned_rows: Vec<Vec<&[f64]>> = (0..layers.saturating_sub(1))
                .map(|l| {
                    (0..widths[l])
                        .map(|r| &TRANSITIONS[l][r][..widths[l + 1]])
                        .collect()
                })
                .collect();
            let transitions_raw: Vec<&[&[f64]]> = owned_rows.iter().map(Vec::as_slice).collect();

            let locals = locals_of(&locals_raw);
            let transitions = transitions_of(&transitions_raw);
            let p = policy();
            let problem = LayeredProblem {
                locals: &locals,
                transitions: &transitions,
                policy: &p,
            };

            if widths.iter().any(|w| *w != widths[0]) {
                ragged_checked += 1;
            }
            for min_distance in 1..=layers {
                for k in 1..=5_usize {
                    let engine = solve_k_best(&problem, request(k, min_distance))
                        .expect("a well-formed problem solves");
                    let oracle = oracle_k_best(&locals_raw, &transitions_raw, k, min_distance);

                    assert_eq!(
                        ordinals_of(&engine),
                        oracle,
                        "widths {widths:?}, k {k}, distance {min_distance}"
                    );
                    for (path, expected) in engine.paths.iter().zip(oracle.iter()) {
                        assert_eq!(
                            path.total_cost.to_bits(),
                            oracle_cost(expected, &locals_raw, &transitions_raw).to_bits(),
                            "totals match bit for bit under the canonical association"
                        );
                    }
                    shapes_checked += 1;
                }
            }
        }
    }
    assert!(shapes_checked >= 1000, "the sweep actually ran: {shapes_checked}");
    assert!(
        ragged_checked >= 100,
        "and it really did cover layers of differing width: {ragged_checked}"
    );
}

// ── the diversity rule is explicit and enforced ──────────────────────────────

#[test]
fn every_pair_of_alternatives_honours_the_requested_distance() {
    let (locals, transitions) = spread_problem();
    let p = policy();
    for min_distance in 1..=4_usize {
        let solution = solve_k_best(
            &LayeredProblem {
                locals: &locals,
                transitions: &transitions,
                policy: &p,
            },
            request(5, min_distance),
        )
        .expect("solves");

        let paths = ordinals_of(&solution);
        for (i, a) in paths.iter().enumerate() {
            for b in paths.iter().skip(i + 1) {
                assert!(
                    distance(a, b) >= min_distance,
                    "distance {min_distance}: {a:?} and {b:?} differ in {} layers",
                    distance(a, b)
                );
            }
        }
    }
}

#[test]
fn a_wider_distance_skips_the_near_clone_the_plain_ranking_would_return() {
    // The whole point of the rule. At distance 1 the runner-up is the optimum
    // with one layer nudged; at distance 2 that path is refused and a genuinely
    // different route takes its place.
    let (locals, transitions) = spread_problem();
    let p = policy();
    let problem = LayeredProblem {
        locals: &locals,
        transitions: &transitions,
        policy: &p,
    };

    let plain = ordinals_of(&solve_k_best(&problem, request(2, 1)).expect("solves"));
    let diverse = ordinals_of(&solve_k_best(&problem, request(2, 2)).expect("solves"));

    assert_eq!(
        plain[0], diverse[0],
        "the optimum is the optimum either way"
    );
    assert_eq!(
        distance(&plain[0], &plain[1]),
        1,
        "the plain runner-up really is a one-layer clone"
    );
    assert!(
        distance(&diverse[0], &diverse[1]) >= 2,
        "the diverse runner-up is a different route"
    );
    assert_ne!(plain[1], diverse[1], "the rule changed the answer");
}

// ── tie-breaking is Slice A's, applied to the whole set ──────────────────────

#[test]
fn equal_cost_alternatives_are_ordered_lexicographically() {
    // Every path costs the same, so nothing but the tie-break decides the order.
    let locals = locals_of(&[&[0.0, 0.0], &[0.0, 0.0]]);
    let transitions = transitions_of(&[&[&[0.0, 0.0], &[0.0, 0.0]]]);
    let p = policy();
    let solution = solve_k_best(
        &LayeredProblem {
            locals: &locals,
            transitions: &transitions,
            policy: &p,
        },
        request(4, 1),
    )
    .expect("solves");

    assert_eq!(
        ordinals_of(&solution),
        vec![vec![0, 0], vec![0, 1], vec![1, 0], vec![1, 1]],
        "an exact tie orders by the ordinal vector, smallest first"
    );
}

// ── honest shortfall: fewer than k, never padded ─────────────────────────────

#[test]
fn a_problem_with_fewer_paths_than_k_reports_exhaustion() {
    // Two layers of two states hold four paths; asking for ten cannot invent six.
    let locals = locals_of(&[&[1.0, 2.0], &[3.0, 4.0]]);
    let transitions = transitions_of(&[&[&[1.0, 2.0], &[2.0, 1.0]]]);
    let p = policy();
    let solution = solve_k_best(
        &LayeredProblem {
            locals: &locals,
            transitions: &transitions,
            policy: &p,
        },
        request(10, 1),
    )
    .expect("solves");

    assert_eq!(solution.paths.len(), 4, "every path, and no invented ones");
    assert!(
        solution.exhausted,
        "the shortfall is reported, not left for the caller to infer from a count"
    );
}

#[test]
fn a_distance_no_second_path_can_meet_returns_one_alternative() {
    // Two layers, two states, distance 2: only the optimum's exact opposite
    // qualifies, so at most two alternatives exist however large k is.
    let locals = locals_of(&[&[1.0, 2.0], &[3.0, 4.0]]);
    let transitions = transitions_of(&[&[&[1.0, 2.0], &[2.0, 1.0]]]);
    let p = policy();
    let solution = solve_k_best(
        &LayeredProblem {
            locals: &locals,
            transitions: &transitions,
            policy: &p,
        },
        request(5, 2),
    )
    .expect("solves");

    assert!(solution.paths.len() <= 2);
    assert!(solution.exhausted, "the search space really did run out");
    let paths = ordinals_of(&solution);
    for pair in paths.windows(2) {
        assert_eq!(distance(&pair[0], &pair[1]), 2);
    }
}

#[test]
fn a_satisfied_request_is_not_marked_exhausted() {
    let (locals, transitions) = spread_problem();
    let p = policy();
    let solution = solve_k_best(
        &LayeredProblem {
            locals: &locals,
            transitions: &transitions,
            policy: &p,
        },
        request(3, 1),
    )
    .expect("solves");

    assert_eq!(solution.paths.len(), 3);
    assert!(
        !solution.exhausted,
        "k was met, so the space was not exhausted"
    );
}

// ── every alternative explains itself completely ─────────────────────────────

#[test]
fn each_alternative_carries_its_full_trace_and_a_total_derived_from_it() {
    let (locals, transitions) = spread_problem();
    let p = policy();
    let solution = solve_k_best(
        &LayeredProblem {
            locals: &locals,
            transitions: &transitions,
            policy: &p,
        },
        request(5, 1),
    )
    .expect("solves");

    for path in &solution.paths {
        assert_eq!(path.steps.len(), 4, "one scored state per layer");
        assert_eq!(path.edges.len(), 3, "one scored edge per adjacent pair");
        for step in &path.steps {
            assert_eq!(
                step.rationale.entries().len(),
                1,
                "the local axis is retained"
            );
        }
        for edge in &path.edges {
            assert_eq!(
                edge.rationale.entries().len(),
                1,
                "the transition axis is retained"
            );
        }
        assert_eq!(
            path.provenance.policy_id, "test_k_best",
            "the policy travels with every alternative, not just the winner"
        );
        assert_eq!(path.provenance.policy_version, 1);
        assert_eq!(
            path.provenance.seed, None,
            "selection uses no seed, here as in Slice A"
        );

        // The total is the canonical fold of this path's own trace.
        let mut back = path.steps.iter().rev();
        let mut expected = back.next().expect("a step").aggregate();
        for (step, edge) in back.zip(path.edges.iter().rev()) {
            expected = edge.aggregate() + expected;
            expected = step.aggregate() + expected;
        }
        assert_eq!(
            path.total_cost.to_bits(),
            expected.to_bits(),
            "total_cost is the trace folded under PATH_COST_ASSOCIATION"
        );
    }
}

// ── stable identity for S8 display and S9 feedback ───────────────────────────

#[test]
fn the_same_problem_yields_the_same_set_every_time() {
    let (locals, transitions) = spread_problem();
    let p = policy();
    let problem = LayeredProblem {
        locals: &locals,
        transitions: &transitions,
        policy: &p,
    };

    let first = solve_k_best(&problem, request(5, 2)).expect("solves");
    let again = solve_k_best(&problem, request(5, 2)).expect("solves");

    assert_eq!(ordinals_of(&first), ordinals_of(&again));
    let bits = |solution: &KBestSolution| -> Vec<u64> {
        solution
            .paths
            .iter()
            .map(|path| path.total_cost.to_bits())
            .collect()
    };
    assert_eq!(bits(&first), bits(&again), "identical to the last bit");
}

#[test]
fn an_alternative_is_identified_by_its_ordinals_not_its_rank() {
    // S9 records which alternative a human chose. Rank is not that identity: the
    // same route sits at a different index once the diversity rule changes,
    // while its ordinal vector does not move.
    let (locals, transitions) = spread_problem();
    let p = policy();
    let problem = LayeredProblem {
        locals: &locals,
        transitions: &transitions,
        policy: &p,
    };

    let narrow = ordinals_of(&solve_k_best(&problem, request(5, 1)).expect("solves"));
    let wide = ordinals_of(&solve_k_best(&problem, request(5, 2)).expect("solves"));

    let moved = wide.iter().find(|path| {
        narrow
            .iter()
            .position(|n| n == *path)
            .is_some_and(|i| wide.iter().position(|w| w == *path) != Some(i))
    });
    assert!(
        moved.is_some(),
        "at least one route survives both rules at a different rank, \
         which is exactly why rank cannot be the identity"
    );
}

// ── refusals: an ask that cannot mean anything is not quietly obliged ────────

#[test]
fn zero_alternatives_is_refused_rather_than_answered_with_nothing() {
    let (locals, transitions) = spread_problem();
    let p = policy();
    assert_eq!(
        solve_k_best(
            &LayeredProblem {
                locals: &locals,
                transitions: &transitions,
                policy: &p,
            },
            request(0, 1),
        )
        .expect_err("an impossible ask is refused"),
        PathError::KZero
    );
}

#[test]
fn a_zero_distance_is_refused_because_it_would_admit_a_clone() {
    let (locals, transitions) = spread_problem();
    let p = policy();
    assert_eq!(
        solve_k_best(
            &LayeredProblem {
                locals: &locals,
                transitions: &transitions,
                policy: &p,
            },
            request(3, 0),
        )
        .expect_err("an impossible ask is refused"),
        PathError::MinDistanceZero
    );
}

#[test]
fn a_distance_wider_than_the_problem_is_refused_as_unsatisfiable() {
    // Four layers cannot hold two paths differing in five of them. Returning a
    // lone path would answer a question the caller did not ask.
    let (locals, transitions) = spread_problem();
    let p = policy();
    assert_eq!(
        solve_k_best(
            &LayeredProblem {
                locals: &locals,
                transitions: &transitions,
                policy: &p,
            },
            request(3, 5),
        )
        .expect_err("an unsatisfiable rule is refused"),
        PathError::MinDistanceUnsatisfiable {
            min_distance: 5,
            layers: 4,
        }
    );
}

#[test]
fn slice_a_s_refusals_still_refuse() {
    // k-best does not become a second, laxer door into the engine.
    let p = policy();
    let empty: Vec<Vec<Axes>> = Vec::new();
    let no_transitions: Vec<Vec<Vec<Axes>>> = Vec::new();
    assert_eq!(
        solve_k_best(
            &LayeredProblem {
                locals: &empty,
                transitions: &no_transitions,
                policy: &p,
            },
            request(3, 1),
        )
        .expect_err("a malformed problem is refused"),
        PathError::NoLayers
    );

    let ragged = locals_of(&[&[1.0], &[1.0]]);
    assert_eq!(
        solve_k_best(
            &LayeredProblem {
                locals: &ragged,
                transitions: &no_transitions,
                policy: &p,
            },
            request(3, 1),
        )
        .expect_err("a malformed problem is refused"),
        PathError::TransitionCount {
            expected: 1,
            found: 0,
        }
    );

    let with_nan = locals_of(&[&[f64::NAN], &[1.0]]);
    let one_edge = transitions_of(&[&[&[1.0]]]);
    assert!(matches!(
        solve_k_best(
            &LayeredProblem {
                locals: &with_nan,
                transitions: &one_edge,
                policy: &p,
            },
            request(3, 1),
        )
        .expect_err("a non-finite cost is refused"),
        PathError::NonFiniteLocal { .. }
    ));
}

#[test]
fn an_enumerated_path_whose_fold_overflows_is_refused_not_queued() {
    // Every input is finite, and `solve` is content: every accumulation the
    // backward DP forms stays finite, because it only ever adds a local to the
    // *cheapest* completion. A non-optimal enumerated path is not so lucky —
    // `[0, 1, 0]` folds to `B + (0 + B)`, which leaves f64 — and that value is
    // a heap key, ordering alternatives by a number no path can report.
    //
    // The rejection rule here is what makes the defect visible at all: the
    // overflowing candidate is two layers from the optimum, so it is discarded
    // before `assemble` could catch it, and `k` is met from finite paths. A
    // laxer accumulation path than `solve`'s therefore returns a clean answer
    // while an infinity sat in the queue.
    const B: f64 = 0.9e308;
    let locals = locals_of(&[&[B, B], &[0.0, B], &[0.0, 0.0]]);
    let transitions = transitions_of(&[&[&[0.0, 0.0], &[0.0, 0.0]], &[&[0.0, 0.0], &[0.0, 0.0]]]);
    let p = policy();
    let problem = LayeredProblem {
        locals: &locals,
        transitions: &transitions,
        policy: &p,
    };

    solve(&problem).expect("the single-path engine finds a finite optimum here");

    assert!(
        matches!(
            solve_k_best(&problem, request(2, 2))
                .expect_err("an accumulation that leaves f64 is refused, as in Slice A"),
            PathError::NonFiniteAccumulation { .. }
        ),
        "k-best must not have a laxer arithmetic than the engine it extends"
    );
}

#[test]
fn the_request_travels_with_the_answer() {
    // S8 shows a set of alternatives; the rule that produced it is part of what
    // the set means, so it is not left at the call site.
    let (locals, transitions) = spread_problem();
    let p = policy();
    let solution = solve_k_best(
        &LayeredProblem {
            locals: &locals,
            transitions: &transitions,
            policy: &p,
        },
        request(3, 2),
    )
    .expect("solves");

    assert_eq!(solution.request.k, 3);
    assert_eq!(solution.request.min_distance, 2);
}
