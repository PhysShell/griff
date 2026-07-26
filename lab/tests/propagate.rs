//! Red → contract tests for the propagating solver (`solve_propagate`).
//!
//! The exact backtracking `solve_exact` is the differential oracle: a
//! propagation-based solver must return the **same** SAT/UNSAT outcome on every
//! problem, and every SAT witness it emits must satisfy the problem. This suite
//! pins that agreement on the named Constraint-Inventory fixtures and on an
//! exhaustive family of small problems (every constraint kind, including the
//! `BandOverlapAtMost` global whose band grows as variables bind — the case
//! plain backtracking's partial prune handles least). Written red-first; the
//! green implementation re-enters through this suite.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::missing_assert_message,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use griff_constraint_lab::{
    ir::{Constraint, IntVar, OracleProblem, VarId},
    problems::bounded_travel_problem,
    solve::{solve_exact, solve_propagate, verify_witness, Outcome},
};
use griff_core::{
    event::{Pitch, Tuning},
    fretboard::STANDARD_MAX_FRET,
};

fn is_sat(o: &Outcome) -> bool {
    matches!(o, Outcome::Sat { .. })
}

fn nodes(o: &Outcome) -> u64 {
    match o {
        Outcome::Sat { nodes, .. } | Outcome::Unsat { nodes } => *nodes,
    }
}

fn pitch(p: u8) -> Pitch {
    Pitch::new(p).expect("valid pitch")
}

fn std_e() -> Tuning {
    Tuning::standard_e()
}

/// The oracle law: `solve_propagate` agrees with `solve_exact` on
/// satisfiability, and any witness it returns is valid.
fn assert_agrees(problem: &OracleProblem) {
    let reference = solve_exact(problem);
    let propagated = solve_propagate(problem);
    assert_eq!(
        is_sat(&propagated),
        is_sat(&reference),
        "SAT/UNSAT disagreement on \"{}\": exact={reference:?} propagate={propagated:?}",
        problem.name()
    );
    if let Outcome::Sat { witness, .. } = &propagated {
        assert!(
            verify_witness(problem, witness),
            "propagate emitted an invalid witness {witness:?} for \"{}\"",
            problem.name()
        );
    }
}

// ── named fixtures (the oracle-spike frontier) ────────────────────────────────

/// The E-minor run spanning [E2 … B4] — the spike's calibration line.
fn e_minor_line() -> Vec<Pitch> {
    [40, 47, 52, 55, 59, 64, 67, 71].map(pitch).to_vec()
}

#[test]
fn agrees_on_bounded_travel_unsat_frontier() {
    // bound 2 is provably UNSAT (spike finding); propagation must also say UNSAT.
    let p =
        bounded_travel_problem(&e_minor_line(), &std_e(), STANDARD_MAX_FRET, 2).expect("builds");
    assert!(
        !is_sat(&solve_exact(&p)),
        "reference sanity: bound 2 is UNSAT"
    );
    assert_agrees(&p);
}

#[test]
fn agrees_on_bounded_travel_sat_frontier() {
    let p =
        bounded_travel_problem(&e_minor_line(), &std_e(), STANDARD_MAX_FRET, 3).expect("builds");
    assert!(is_sat(&solve_exact(&p)), "reference sanity: bound 3 is SAT");
    assert_agrees(&p);
}

#[test]
fn agrees_on_bounded_travel_loose() {
    let p =
        bounded_travel_problem(&e_minor_line(), &std_e(), STANDARD_MAX_FRET, 24).expect("builds");
    assert_agrees(&p);
}

// ── determinism ───────────────────────────────────────────────────────────────

#[test]
fn is_deterministic() {
    let p =
        bounded_travel_problem(&e_minor_line(), &std_e(), STANDARD_MAX_FRET, 3).expect("builds");
    assert_eq!(solve_propagate(&p), solve_propagate(&p));
}

// ── exhaustive differential harness ───────────────────────────────────────────

/// Every non-empty subset of `{0,1,2,3}`, as a domain.
fn small_domains() -> Vec<Vec<i64>> {
    let mut out = Vec::new();
    for mask in 1u8..16 {
        let d: Vec<i64> = (0i64..4).filter(|&v| mask & (1 << v) != 0).collect();
        out.push(d);
    }
    out
}

#[test]
fn agrees_on_all_small_absdiff_problems() {
    let doms = small_domains();
    let mut checked = 0u32;
    for da in &doms {
        for db in &doms {
            for bound in 0i64..=3 {
                let p = OracleProblem::new(
                    "absdiff",
                    vec![IntVar::new("x", da.clone()), IntVar::new("y", db.clone())],
                    vec![Constraint::AbsDiffLe {
                        a: VarId(0),
                        b: VarId(1),
                        bound,
                    }],
                );
                assert_agrees(&p);
                checked += 1;
            }
        }
    }
    assert!(
        checked >= 900,
        "expected the full small grid, got {checked}"
    );
}

#[test]
fn agrees_on_all_small_band_problems() {
    // The hard case: a whole-band global whose band is the min/max over vars,
    // overlap measured relative to the narrower band, with the degenerate
    // single-point rule. Two band members here (three-member and mixed cases
    // live in `agrees_on_three_var_band_and_travel_chain`), several thresholds
    // and fixed bands.
    let doms = small_domains();
    let fixed_bands = [(0, 0), (1, 1), (0, 3), (1, 2)];
    let thresholds = [(0, 1), (1, 2), (1, 1)]; // <=0, <=1/2, <=1
    let mut checked = 0u32;
    for da in &doms {
        for db in &doms {
            for &(flo, fhi) in &fixed_bands {
                for &(num, den) in &thresholds {
                    let p = OracleProblem::new(
                        "band",
                        vec![IntVar::new("x", da.clone()), IntVar::new("y", db.clone())],
                        vec![Constraint::BandOverlapAtMost {
                            vars: vec![VarId(0), VarId(1)],
                            fixed_lo: flo,
                            fixed_hi: fhi,
                            num,
                            den,
                        }],
                    );
                    assert_agrees(&p);
                    checked += 1;
                }
            }
        }
    }
    assert!(checked > 2000, "expected the full band grid, got {checked}");
}

#[test]
fn agrees_on_combined_absdiff_and_forbidden_classes() {
    let doms = small_domains();
    for da in &doms {
        for db in &doms {
            let p = OracleProblem::new(
                "combo",
                vec![IntVar::new("x", da.clone()), IntVar::new("y", db.clone())],
                vec![
                    Constraint::AbsDiffLe {
                        a: VarId(0),
                        b: VarId(1),
                        bound: 2,
                    },
                    Constraint::ForbiddenIntervalClasses {
                        var: VarId(0),
                        fixed: 0,
                        classes: vec![1, 6, 11],
                    },
                ],
            );
            assert_agrees(&p);
        }
    }
}

// ── propagation actually prunes (not exact-backtracking renamed) ──────────────
//
// Agreement alone is satisfied by `solve_propagate = solve_exact`. These pin the
// distinguishing property: on constructed fixtures the forward-checked search
// explores *strictly fewer* nodes than the reference, while still returning the
// same outcome, a valid witness, and a deterministic result.

#[test]
fn prunes_more_than_reference_on_unsat_disjoint_bound() {
    // x ∈ {0..3}, y ∈ {10..13}, |x - y| ≤ 0 ⇒ UNSAT. Forward checking wipes out
    // y at every x without descending; plain backtracking tries every (x, y).
    let p = OracleProblem::new(
        "unsat_disjoint",
        vec![
            IntVar::new("x", vec![0, 1, 2, 3]),
            IntVar::new("y", vec![10, 11, 12, 13]),
        ],
        vec![Constraint::AbsDiffLe {
            a: VarId(0),
            b: VarId(1),
            bound: 0,
        }],
    );
    let exact = solve_exact(&p);
    let prop = solve_propagate(&p);
    assert!(!is_sat(&exact) && !is_sat(&prop), "both UNSAT");
    assert!(
        nodes(&prop) < nodes(&exact),
        "propagation must prune: exact={} prop={}",
        nodes(&exact),
        nodes(&prop)
    );
    assert_eq!(prop, solve_propagate(&p), "deterministic");
}

#[test]
fn prunes_more_than_reference_on_sat_with_doomed_branch() {
    // x ∈ {0,1}, y ∈ {1,5}, x == y. Only (1,1) works. Forward checking rejects
    // x=0 without trying any y; the reference descends into y under x=0 first.
    let p = OracleProblem::new(
        "sat_doomed_branch",
        vec![IntVar::new("x", vec![0, 1]), IntVar::new("y", vec![1, 5])],
        vec![Constraint::AbsDiffLe {
            a: VarId(0),
            b: VarId(1),
            bound: 0,
        }],
    );
    let exact = solve_exact(&p);
    let prop = solve_propagate(&p);
    assert!(is_sat(&exact) && is_sat(&prop), "both SAT");
    assert!(verify_witness(
        &p,
        match &prop {
            Outcome::Sat { witness, .. } => witness,
            Outcome::Unsat { .. } => unreachable!(),
        }
    ));
    assert!(
        nodes(&prop) < nodes(&exact),
        "propagation must prune: exact={} prop={}",
        nodes(&exact),
        nodes(&prop)
    );
    assert_eq!(prop, solve_propagate(&p), "deterministic");
}

// ── overflow safety on extreme i64 (the IR advertises arbitrary i64) ──────────
//
// The travel-bound and band arithmetic must not overflow for extreme domain
// values. The order matters: `solve_propagate` evaluates the travel bound in
// forward checking *before* the partial band/other checks, so a naive
// `(x - y).abs()` panicked here on a problem the reference resolved without ever
// touching that subtraction. Each of these returns cleanly and agrees.

#[test]
fn no_overflow_when_reference_rejects_before_the_travel_bound() {
    // a = i64::MIN, b = 0. A degenerate band over b rejects b in the reference's
    // partial check *before* the AbsDiffLe(a, b, i64::MAX) term is reached; the
    // propagator forward-checks that AbsDiffLe against the assigned a first, so
    // `0 - i64::MIN` is computed and must not overflow.
    let p = OracleProblem::new(
        "overflow_order",
        vec![IntVar::new("a", vec![i64::MIN]), IntVar::new("b", vec![0])],
        vec![
            Constraint::BandOverlapAtMost {
                vars: vec![VarId(1)],
                fixed_lo: 0,
                fixed_hi: 0,
                num: 0,
                den: 1,
            },
            Constraint::AbsDiffLe {
                a: VarId(0),
                b: VarId(1),
                bound: i64::MAX,
            },
        ],
    );
    assert!(
        !is_sat(&solve_exact(&p)),
        "reference resolves it (UNSAT), no panic"
    );
    assert_agrees(&p);
}

#[test]
fn no_overflow_on_extreme_travel_and_band_and_class() {
    // Extreme values across all three constraint kinds; both solvers must return
    // an Outcome (never overflow-panic) and agree.
    for p in [
        OracleProblem::new(
            "extreme_travel",
            vec![
                IntVar::new("x", vec![i64::MIN, 0, i64::MAX]),
                IntVar::new("y", vec![i64::MIN, i64::MAX]),
            ],
            vec![Constraint::AbsDiffLe {
                a: VarId(0),
                b: VarId(1),
                bound: i64::MAX,
            }],
        ),
        OracleProblem::new(
            "extreme_band",
            vec![
                IntVar::new("x", vec![i64::MIN, i64::MAX]),
                IntVar::new("y", vec![i64::MIN, 0, i64::MAX]),
            ],
            vec![Constraint::BandOverlapAtMost {
                vars: vec![VarId(0), VarId(1)],
                fixed_lo: i64::MIN,
                fixed_hi: i64::MAX,
                num: 1,
                den: 2,
            }],
        ),
        OracleProblem::new(
            "extreme_class",
            vec![IntVar::new("x", vec![i64::MIN, -1, i64::MAX])],
            vec![Constraint::ForbiddenIntervalClasses {
                var: VarId(0),
                fixed: i64::MAX,
                classes: vec![0, 1, 6, 11],
            }],
        ),
    ] {
        assert_agrees(&p);
    }
}

// ── three-variable band + travel chain (the global × forward-checking mix) ────

#[test]
fn agrees_on_three_var_band_and_travel_chain() {
    // The interaction that matters more than another 2-var grid: a whole-band
    // global over three members combined with a chain of travel bounds, so the
    // band's partial prune and the new forward checking act on the same search.
    let doms = [vec![0, 1, 2, 3], vec![0, 2], vec![1, 2, 3]];
    for da in &doms {
        for db in &doms {
            for dc in &doms {
                for &(num, den) in &[(0_i64, 1_i64), (1, 2), (1, 1)] {
                    let p = OracleProblem::new(
                        "band3_chain",
                        vec![
                            IntVar::new("x", da.clone()),
                            IntVar::new("y", db.clone()),
                            IntVar::new("z", dc.clone()),
                        ],
                        vec![
                            Constraint::AbsDiffLe {
                                a: VarId(0),
                                b: VarId(1),
                                bound: 1,
                            },
                            Constraint::AbsDiffLe {
                                a: VarId(1),
                                b: VarId(2),
                                bound: 1,
                            },
                            Constraint::BandOverlapAtMost {
                                vars: vec![VarId(0), VarId(1), VarId(2)],
                                fixed_lo: 1,
                                fixed_hi: 2,
                                num,
                                den,
                            },
                        ],
                    );
                    assert_agrees(&p);
                }
            }
        }
    }
}
