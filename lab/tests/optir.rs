//! Red → contract tests for the solver-neutral optimization IR (`optir`).
//!
//! Pins: construction refusals and canonical form, exact re-scoring of every
//! term kind (the only authority on a witness), the wire shape an external
//! solver adapter consumes and produces, and the verdict rules that decide
//! when a solver's optimality claim is accepted.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    clippy::missing_assert_message,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use griff_constraint_lab::{
    ir::{IntVar, IrError, VarId},
    manifest::SolverIdentity,
    optir::{
        verify_agreement, verify_record, AgreementError, AgreementRecord, Hard, OptIrError,
        OptProblem, ProblemRecord, SolveRecord, SolveStatus, Term, Verdict, WitnessError,
        OPT_SCHEMA, OPT_SCHEMA_VERSION,
    },
};

/// x ∈ {0,1,2}, y ∈ {0,2,5}; (x, y) ∈ {(0,0),(1,2),(2,5),(2,2)};
/// cost = unary(x: 1→4, 2→-3) + pair(x,y: (1,2)→7) + 2·|x−y| + 5·[x≠y].
fn sample() -> OptProblem {
    OptProblem::try_new(
        "sample",
        vec![
            IntVar::new("x", vec![2, 0, 1]),
            IntVar::new("y", vec![5, 0, 2]),
        ],
        vec![Hard::Allowed {
            a: VarId(0),
            b: VarId(1),
            tuples: vec![(2, 5), (0, 0), (1, 2), (2, 2)],
        }],
        vec![
            Term::Unary {
                var: VarId(0),
                costs: vec![(2, -3), (1, 4)],
            },
            Term::Pair {
                a: VarId(0),
                b: VarId(1),
                costs: vec![(1, 2, 7)],
            },
            Term::AbsDiff {
                a: VarId(0),
                b: VarId(1),
                weight: 2,
            },
            Term::NotEqual {
                a: VarId(0),
                b: VarId(1),
                weight: 5,
            },
        ],
    )
    .expect("valid sample")
}

fn solver() -> SolverIdentity {
    SolverIdentity {
        name: "test".into(),
        version: "0".into(),
    }
}

fn record(
    problem: &OptProblem,
    status: SolveStatus,
    objective: i64,
    witness: Vec<i64>,
) -> SolveRecord {
    SolveRecord {
        id: "r".into(),
        fingerprint_hex: format!("{:016x}", problem.fingerprint()),
        solver: solver(),
        status,
        objective: Some(objective),
        bound: Some(objective),
        witness: Some(witness),
        wall_us: 1,
        agreement: None,
    }
}

// ── construction ──────────────────────────────────────────────────────────────

#[test]
fn refuses_dangling_ids_in_hard_and_objective() {
    let vars = || vec![IntVar::new("x", vec![0, 1])];
    let hard = OptProblem::try_new(
        "p",
        vars(),
        vec![Hard::Allowed {
            a: VarId(0),
            b: VarId(3),
            tuples: vec![(0, 0)],
        }],
        vec![],
    );
    assert_eq!(
        hard,
        Err(OptIrError::Ir(IrError::DanglingVarId { id: 3, vars: 1 }))
    );
    for term in [
        Term::Unary {
            var: VarId(1),
            costs: vec![],
        },
        Term::Pair {
            a: VarId(0),
            b: VarId(1),
            costs: vec![],
        },
        Term::AbsDiff {
            a: VarId(1),
            b: VarId(0),
            weight: 1,
        },
        Term::NotEqual {
            a: VarId(0),
            b: VarId(1),
            weight: 1,
        },
    ] {
        assert_eq!(
            OptProblem::try_new("p", vars(), vec![], vec![term]),
            Err(OptIrError::Ir(IrError::DanglingVarId { id: 1, vars: 1 }))
        );
    }
}

#[test]
fn refuses_shared_ir_violations() {
    assert!(matches!(
        OptProblem::try_new("p", vec![IntVar::new("x", vec![])], vec![], vec![]),
        Err(OptIrError::Ir(IrError::EmptyDomain { .. }))
    ));
    assert!(matches!(
        OptProblem::try_new(
            "p",
            vec![IntVar::new("x", vec![0]), IntVar::new("x", vec![1])],
            vec![],
            vec![]
        ),
        Err(OptIrError::Ir(IrError::DuplicateName { .. }))
    ));
    assert!(matches!(
        OptProblem::try_new("p", vec![IntVar::new("1x", vec![0])], vec![], vec![]),
        Err(OptIrError::Ir(IrError::UnsafeName { .. }))
    ));
}

#[test]
fn refuses_ambiguous_cost_tables_and_empty_hard_tables() {
    let vars = || vec![IntVar::new("x", vec![0, 1]), IntVar::new("y", vec![0, 1])];
    assert_eq!(
        OptProblem::try_new(
            "p",
            vars(),
            vec![],
            vec![
                Term::AbsDiff {
                    a: VarId(0),
                    b: VarId(1),
                    weight: 1
                },
                Term::Unary {
                    var: VarId(0),
                    costs: vec![(1, 2), (1, 3)]
                }
            ]
        ),
        Err(OptIrError::DuplicateCostKey { term: 1 })
    );
    assert_eq!(
        OptProblem::try_new(
            "p",
            vars(),
            vec![],
            vec![Term::Pair {
                a: VarId(0),
                b: VarId(1),
                costs: vec![(0, 1, 2), (0, 1, 2)]
            }]
        ),
        Err(OptIrError::DuplicateCostKey { term: 0 })
    );
    assert_eq!(
        OptProblem::try_new(
            "p",
            vars(),
            vec![
                Hard::Allowed {
                    a: VarId(0),
                    b: VarId(1),
                    tuples: vec![(0, 0)]
                },
                Hard::Allowed {
                    a: VarId(0),
                    b: VarId(1),
                    tuples: vec![]
                }
            ],
            vec![]
        ),
        Err(OptIrError::EmptyAllowedTable { index: 1 })
    );
}

#[test]
fn tables_are_canonical_so_fingerprints_ignore_input_order() {
    let reordered = OptProblem::try_new(
        "sample",
        vec![
            IntVar::new("x", vec![0, 1, 2]),
            IntVar::new("y", vec![0, 2, 5]),
        ],
        vec![Hard::Allowed {
            a: VarId(0),
            b: VarId(1),
            tuples: vec![(0, 0), (2, 2), (1, 2), (2, 5), (0, 0)],
        }],
        vec![
            Term::Unary {
                var: VarId(0),
                costs: vec![(1, 4), (2, -3)],
            },
            Term::Pair {
                a: VarId(0),
                b: VarId(1),
                costs: vec![(1, 2, 7)],
            },
            Term::AbsDiff {
                a: VarId(0),
                b: VarId(1),
                weight: 2,
            },
            Term::NotEqual {
                a: VarId(0),
                b: VarId(1),
                weight: 5,
            },
        ],
    )
    .expect("valid");
    assert_eq!(reordered, sample());
    assert_eq!(reordered.fingerprint(), sample().fingerprint());
    let canonical = sample();
    let Hard::Allowed { tuples, .. } = &canonical.hard()[0];
    assert_eq!(tuples, &vec![(0, 0), (1, 2), (2, 2), (2, 5)]);
}

#[test]
fn fingerprint_is_sensitive_to_a_weight() {
    let base = sample();
    let mut objective = base.objective().to_vec();
    objective[2] = Term::AbsDiff {
        a: VarId(0),
        b: VarId(1),
        weight: 3,
    };
    let changed = OptProblem::try_new(
        base.name(),
        base.vars().to_vec(),
        base.hard().to_vec(),
        objective,
    )
    .expect("valid");
    assert_ne!(changed.fingerprint(), base.fingerprint());
}

// ── evaluation ────────────────────────────────────────────────────────────────

#[test]
fn evaluates_every_term_kind_exactly() {
    let p = sample();
    // (0,0): unary 0 (absent) + pair 0 + 2·0 + 5·0 = 0
    assert_eq!(p.evaluate(&[0, 0]), Ok(0));
    // (1,2): unary 4 + pair 7 + 2·1 + 5·1 = 18
    assert_eq!(p.evaluate(&[1, 2]), Ok(18));
    // (2,5): unary −3 + pair 0 + 2·3 + 5·1 = 8
    assert_eq!(p.evaluate(&[2, 5]), Ok(8));
    // (2,2): unary −3 + 0 + 0 + 0 = −3
    assert_eq!(p.evaluate(&[2, 2]), Ok(-3));
}

#[test]
fn evaluation_refuses_inadmissible_witnesses() {
    let p = sample();
    assert_eq!(
        p.evaluate(&[0]),
        Err(WitnessError::Length {
            expected: 2,
            got: 1
        })
    );
    assert_eq!(
        p.evaluate(&[3, 0]),
        Err(WitnessError::OutOfDomain {
            name: "x".into(),
            value: 3
        })
    );
    assert_eq!(
        p.evaluate(&[0, 2]),
        Err(WitnessError::HardViolated { index: 0 })
    );
}

#[test]
fn evaluation_refuses_overflow() {
    let p = OptProblem::try_new(
        "big",
        vec![
            IntVar::new("x", vec![i64::MIN, i64::MAX]),
            IntVar::new("y", vec![i64::MIN, i64::MAX]),
        ],
        vec![],
        vec![Term::AbsDiff {
            a: VarId(0),
            b: VarId(1),
            weight: 2,
        }],
    )
    .expect("valid");
    assert_eq!(
        p.evaluate(&[i64::MIN, i64::MAX]),
        Err(WitnessError::Overflow)
    );
    assert_eq!(p.evaluate(&[i64::MAX, i64::MAX]), Ok(0));
}

// ── wire contract ─────────────────────────────────────────────────────────────

#[test]
fn problem_record_wire_shape_is_pinned() {
    let p = sample();
    let fp = p.fingerprint();
    let rec = ProblemRecord::new("line-7", p, vec![(VarId(0), 2)]);
    assert_eq!(rec.fingerprint_hex, format!("{fp:016x}"));
    let json: serde_json::Value = serde_json::to_value(&rec).expect("serializes");
    assert_eq!(json["schema"], OPT_SCHEMA);
    assert_eq!(json["version"], OPT_SCHEMA_VERSION);
    assert_eq!(json["id"], "line-7");
    assert_eq!(json["problem"]["vars"][0]["name"], "x");
    assert_eq!(
        json["problem"]["vars"][0]["domain"],
        serde_json::json!([0, 1, 2])
    );
    assert_eq!(json["problem"]["hard"][0]["kind"], "allowed");
    assert_eq!(
        json["problem"]["hard"][0]["tuples"][1],
        serde_json::json!([1, 2])
    );
    let kinds: Vec<&str> = json["problem"]["objective"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["kind"].as_str().unwrap())
        .collect();
    assert_eq!(kinds, ["unary", "pair", "abs_diff", "not_equal"]);
    assert_eq!(
        json["problem"]["objective"][1]["costs"][0],
        serde_json::json!([1, 2, 7])
    );
    assert_eq!(json["reference"], serde_json::json!([[0, 2]]));
}

#[test]
fn solve_record_parses_the_adapter_output() {
    let line = r#"{"id":"line-7","fingerprint_hex":"00000000000000ff",
        "solver":{"name":"ortools/cp-sat","version":"9.15.6755"},
        "status":"optimal","objective":-3,"bound":-3,"witness":[2,2],"wall_us":1234,
        "agreement":{"status":"optimal","matched":1,"witness":[2,2]}}"#;
    let rec: SolveRecord = serde_json::from_str(line).expect("parses");
    assert_eq!(rec.status, SolveStatus::Optimal);
    assert_eq!(rec.witness, Some(vec![2, 2]));
    assert_eq!(rec.agreement.unwrap().matched, Some(1));
    let infeasible = r#"{"id":"x","fingerprint_hex":"0","solver":{"name":"s","version":"v"},
        "status":"infeasible","objective":null,"bound":null,"witness":null,"wall_us":0,"agreement":null}"#;
    let rec: SolveRecord = serde_json::from_str(infeasible).expect("parses");
    assert_eq!(rec.status, SolveStatus::Infeasible);
}

// ── verdicts ──────────────────────────────────────────────────────────────────

#[test]
fn verdict_proven_only_for_a_verified_optimal_claim() {
    let p = sample();
    assert_eq!(
        verify_record(&p, &record(&p, SolveStatus::Optimal, -3, vec![2, 2])),
        Verdict::Proven { optimum: -3 }
    );
}

#[test]
fn verdict_refuses_every_broken_claim() {
    let p = sample();
    let mut wrong_fp = record(&p, SolveStatus::Optimal, -3, vec![2, 2]);
    wrong_fp.fingerprint_hex = "0000000000000000".into();
    assert_eq!(verify_record(&p, &wrong_fp), Verdict::FingerprintMismatch);

    assert_eq!(
        verify_record(&p, &record(&p, SolveStatus::Feasible, -3, vec![2, 2])),
        Verdict::NotProven {
            status: SolveStatus::Feasible
        }
    );

    let mut no_witness = record(&p, SolveStatus::Optimal, -3, vec![]);
    no_witness.witness = None;
    assert_eq!(verify_record(&p, &no_witness), Verdict::MissingWitness);

    assert_eq!(
        verify_record(&p, &record(&p, SolveStatus::Optimal, 0, vec![0, 2])),
        Verdict::WitnessInvalid(WitnessError::HardViolated { index: 0 })
    );

    assert_eq!(
        verify_record(&p, &record(&p, SolveStatus::Optimal, 0, vec![2, 2])),
        Verdict::ObjectiveMismatch {
            claimed: 0,
            rescored: -3,
            bound: Some(0)
        }
    );

    let mut loose_bound = record(&p, SolveStatus::Optimal, -3, vec![2, 2]);
    loose_bound.bound = Some(-5);
    assert_eq!(
        verify_record(&p, &loose_bound),
        Verdict::ObjectiveMismatch {
            claimed: -3,
            rescored: -3,
            bound: Some(-5)
        }
    );
}

#[test]
fn agreement_pass_is_recounted_and_pinned_to_the_optimum() {
    let p = sample();
    let reference = vec![(VarId(0), 2), (VarId(1), 5)];
    let ok = AgreementRecord {
        status: SolveStatus::Optimal,
        matched: Some(1),
        witness: Some(vec![2, 2]),
    };
    assert_eq!(verify_agreement(&p, &reference, -3, &ok), Ok(1));

    let off = AgreementRecord {
        status: SolveStatus::Optimal,
        matched: Some(2),
        witness: Some(vec![2, 5]),
    };
    assert_eq!(
        verify_agreement(&p, &reference, -3, &off),
        Err(AgreementError::OffOptimum {
            optimum: -3,
            rescored: 8
        })
    );

    let miscount = AgreementRecord {
        status: SolveStatus::Optimal,
        matched: Some(2),
        witness: Some(vec![2, 2]),
    };
    assert_eq!(
        verify_agreement(&p, &reference, -3, &miscount),
        Err(AgreementError::CountMismatch {
            claimed: 2,
            recounted: 1
        })
    );

    let unproven = AgreementRecord {
        status: SolveStatus::Unknown,
        matched: None,
        witness: None,
    };
    assert_eq!(
        verify_agreement(&p, &reference, -3, &unproven),
        Err(AgreementError::NotProven {
            status: SolveStatus::Unknown
        })
    );
}
