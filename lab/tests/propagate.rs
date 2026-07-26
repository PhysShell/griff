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
    // single-point rule. Two and three band members, several thresholds and
    // fixed bands.
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
