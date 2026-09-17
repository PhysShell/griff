// TDD red: the persistent experiment bundle V1 (ADR-0034 decision 9). The
// four acceptance gates:
//   1. loading never runs a generator or planner;
//   2. the wire form is the one canonical projection, and fingerprints come
//      from it;
//   3. the round trip is lossless, or refuses typed;
//   4. the bundle holds the whole spec and both cell identities, so it explains
//      itself.
#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::missing_assert_message
)]

mod common;

use common::{corpus, score_of, source, two_by_two};
use griff_core::candidate_chain::ChainError;
use griff_core::generation_input::generation_request_from_score;
use griff_core::layered_path::{PathError, StateId};
use griff_experiment::{
    run_experiment, score_fingerprint, BundleError, CellOutcome, CellOutcomeV1, CellRefusal,
    EvaluationContext, ExperimentBundleV1, ExperimentInputs, ExperimentRun, ExperimentSpec,
    InformationRegime, Mismatch, ScoreV1, Stage, BUNDLE_SCHEMA, BUNDLE_VERSION,
};
use serde_json::{json, Value};

fn spec() -> ExperimentSpec {
    two_by_two(EvaluationContext::GenerationAxes {
        pitch_material: generation_request_from_score(&source(), 42, 4)
            .expect("seeds")
            .pitch_material,
        references: vec![score_of(&[(0, 480, 45), (480, 480, 47), (960, 960, 50)])],
    })
}

fn run() -> ExperimentRun {
    let (source, corpus) = (source(), corpus());
    run_experiment(
        &spec(),
        &ExperimentInputs {
            source: &source,
            corpus: Some(&corpus),
        },
    )
    .expect("the fixture runs")
}

fn bundle(run: &ExperimentRun) -> ExperimentBundleV1 {
    ExperimentBundleV1::from_run(&spec(), &source(), run).expect("this run's spec and source")
}

fn json_of(run: &ExperimentRun) -> Value {
    serde_json::from_str(&bundle(run).to_json().expect("serializes")).expect("valid JSON")
}

fn load(value: &Value) -> Result<ExperimentBundleV1, BundleError> {
    ExperimentBundleV1::from_json(&value.to_string())
}

// ── gate 3: lossless ─────────────────────────────────────────────────────────

#[test]
fn a_run_survives_its_bundle_exactly() {
    let run = run();
    let written = bundle(&run);
    let json = written.to_json().expect("serializes");
    let loaded = ExperimentBundleV1::from_json(&json).expect("loads");
    assert_eq!(loaded, written);
    assert_eq!(
        loaded.to_json().expect("serializes"),
        json,
        "writing is deterministic"
    );
    assert_eq!(
        loaded.run(),
        Ok(run.clone()),
        "nothing of the run was left out"
    );
    assert_eq!(loaded.source_score(), Ok(source()));
    assert_eq!(loaded.spec().expect("rebuilds").fingerprint(), run.spec);
}

#[test]
fn a_refused_cell_survives_its_bundle_exactly() {
    let mut run = run();
    run.cells[1].outcome = CellOutcome::Refused(CellRefusal::EmptySet);
    run.cells[3].outcome = CellOutcome::Refused(CellRefusal::Chain(ChainError::Path(
        PathError::NonFiniteLocal {
            state: StateId {
                layer: 2,
                ordinal: 7,
            },
            cost: f64::INFINITY,
        },
    )));
    let loaded =
        ExperimentBundleV1::from_json(&bundle(&run).to_json().expect("serializes")).expect("loads");
    assert_eq!(
        loaded.run(),
        Ok(run),
        "a refusal stays typed, cost bits included"
    );
}

#[test]
fn a_bundle_for_another_spec_or_source_is_refused() {
    let run = run();
    assert_eq!(
        ExperimentBundleV1::from_run(&two_by_two(EvaluationContext::None), &source(), &run),
        Err(BundleError::NotThisRun)
    );
    assert_eq!(
        ExperimentBundleV1::from_run(&spec(), &score_of(&[(0, 480, 60)]), &run),
        Err(BundleError::NotThisRun)
    );
}

// ── gate 4: self-explanatory ─────────────────────────────────────────────────

#[test]
fn the_bundle_holds_the_whole_spec_and_both_cell_identities() {
    let run = run();
    let value = json_of(&run);
    assert_eq!(value["schema"], json!(BUNDLE_SCHEMA));
    assert_eq!(value["version"], json!(BUNDLE_VERSION));
    assert_eq!(value["spec"]["ask"]["seed"], json!(42));
    assert_eq!(value["spec"]["variants"][0]["label"], json!("S6 Intact"));
    assert_eq!(
        value["spec"]["variants"][1]["selector"]["identity"],
        json!({"id": "candidate_chain", "version": 1})
    );
    assert_eq!(
        value["spec"]["evaluation"]["generation_axes"]["references"]
            .as_array()
            .map(Vec::len),
        Some(1),
        "the evaluation context's references are recorded, not only hashed"
    );
    for (i, cell) in run.cells.iter().enumerate() {
        assert_eq!(
            value["cells"][i]["requested"],
            json!(cell.requested.to_hex())
        );
        assert_eq!(value["cells"][i]["recipe"], json!(cell.recipe.to_hex()));
    }

    let rebuilt = ExperimentBundleV1::from_json(&value.to_string())
        .expect("loads")
        .spec()
        .expect("rebuilds");
    assert_eq!(
        rebuilt
            .variants
            .iter()
            .map(|v| v.label.as_str())
            .collect::<Vec<_>>(),
        ["S6 Intact", "S7 Global Chain"]
    );
    assert_eq!(rebuilt.regimes, spec().regimes);
    assert_eq!(rebuilt.fingerprint(), spec().fingerprint());
}

#[test]
fn metric_names_are_owned_strings_and_realization_is_only_absence() {
    let value = json_of(&run());
    let produced = &value["cells"][0]["outcome"]["produced"];
    assert_eq!(produced["realization"], Value::Null);
    let names: Vec<&str> = produced["metrics"]
        .as_array()
        .expect("metrics")
        .iter()
        .filter_map(|m| m["name"].as_str())
        .collect();
    assert!(names.contains(&"internal_continuity") && names.contains(&"chain_cost"));

    for invented in [json!({}), json!("placeholder"), json!({"fingering": []})] {
        let mut forged = value.clone();
        forged["cells"][0]["outcome"]["produced"]["realization"] = invented;
        assert!(
            matches!(load(&forged), Err(BundleError::Malformed(_))),
            "a realization no policy produced cannot be loaded"
        );
    }

    let mut renamed = value;
    renamed["cells"][0]["outcome"]["produced"]["metrics"][0]["name"] = json!("made_up_axis");
    assert_eq!(
        load(&renamed),
        Err(BundleError::IdentityMismatch(Mismatch::CellRecord {
            cell: 0
        })),
        "a metric name is part of what the cell claims"
    );

    // A name outside the vocabulary never becomes a `&'static str`, even in a
    // bundle built in memory rather than loaded.
    let mut in_memory = bundle(&run());
    let CellOutcomeV1::Produced(result) = &mut in_memory.cells[0].outcome else {
        panic!("produced");
    };
    result.metrics[0].name = "made_up_axis".to_owned();
    assert_eq!(
        in_memory.run(),
        Err(BundleError::UnknownName("made_up_axis".to_owned()))
    );
}

// ── gate 1: loading never generates ──────────────────────────────────────────

#[test]
fn loading_shows_the_recorded_score_not_a_regenerated_one() {
    let run = run();
    let mut value = json_of(&run);
    // A recorded score no generator would produce for this ask, with its own
    // content fingerprint: consistent, so it loads — and it is what comes back.
    let foreign = score_of(&[(0, 1920, 64)]);
    value["cells"][0]["outcome"]["produced"]["score"] =
        serde_json::to_value(ScoreV1::from(&foreign)).expect("serializes");
    value["cells"][0]["outcome"]["produced"]["content"] =
        json!(score_fingerprint(&foreign).to_hex());

    let loaded = load(&value).expect("a consistent bundle loads");
    let CellOutcome::Produced(result) = &loaded.run().expect("rebuilds").cells[0].outcome else {
        panic!("produced");
    };
    assert_eq!(result.score, foreign, "shown as recorded");
    let CellOutcome::Produced(generated) = &run.cells[0].outcome else {
        panic!("produced");
    };
    assert_ne!(
        result.score, generated.score,
        "and not as the generator would"
    );
}

// ── gate 2 and verification: every identity recomputes from the bundle ───────

#[test]
fn a_bundle_whose_identities_disagree_with_its_data_is_refused() {
    let base = json_of(&run());
    let refused = |edit: &dyn Fn(&mut Value)| {
        let mut value = base.clone();
        edit(&mut value);
        load(&value).expect_err("refused")
    };

    assert_eq!(
        refused(&|v| v["spec"]["ask"]["seed"] = json!(43)),
        BundleError::IdentityMismatch(Mismatch::Spec)
    );
    assert_eq!(
        refused(&|v| v["source"]["tracks"][0]["channel"] = json!(5)),
        BundleError::IdentityMismatch(Mismatch::Source)
    );
    assert_eq!(
        refused(&|v| v["population"]["skipped"] = json!([])),
        BundleError::IdentityMismatch(Mismatch::Population)
    );
    assert_eq!(
        refused(&|v| v["passes"][1]["information"] = base["passes"][0]["information"].clone()),
        BundleError::IdentityMismatch(Mismatch::PassInformation { pass: 1 })
    );
    assert_eq!(
        refused(&|v| v["cells"][2]["requested"] = base["cells"][0]["requested"].clone()),
        BundleError::IdentityMismatch(Mismatch::CellRequested { cell: 2 })
    );
    assert_eq!(
        refused(&|v| v["cells"][2]["recipe"] = base["cells"][0]["recipe"].clone()),
        BundleError::IdentityMismatch(Mismatch::CellRecipe { cell: 2 })
    );
    assert_eq!(
        refused(&|v| v["cells"][1]["pass"] = json!(0)),
        BundleError::IdentityMismatch(Mismatch::CellPass { cell: 1 }),
        "a cell cannot claim a pass of another regime"
    );
    assert_eq!(
        refused(&|v| {
            v["cells"][0]["outcome"]["produced"]["score"]["tracks"][0]["channel"] = json!(5);
        }),
        BundleError::IdentityMismatch(Mismatch::CellContent { cell: 0 })
    );
    assert_eq!(
        refused(&|v| {
            v["cells"][0]["outcome"]["produced"]["metrics"][0]["context"] =
                base["passes"][1]["information"].clone();
        }),
        BundleError::IdentityMismatch(Mismatch::MetricContext { cell: 0, metric: 0 })
    );
}

#[test]
fn a_bundle_of_another_schema_version_or_shape_is_refused() {
    let base = json_of(&run());
    let mut schema = base.clone();
    schema["schema"] = json!("griff.candidate-provenance");
    assert_eq!(
        load(&schema),
        Err(BundleError::UnknownSchema(
            "griff.candidate-provenance".to_owned()
        ))
    );
    let mut version = base.clone();
    version["version"] = json!(2);
    assert_eq!(load(&version), Err(BundleError::UnsupportedVersion(2)));
    let mut unknown = base.clone();
    unknown["notes"] = json!("hand-written");
    assert!(matches!(load(&unknown), Err(BundleError::Malformed(_))));
    let mut bad_hex = base.clone();
    bad_hex["identities"]["spec"] = json!("not-a-fingerprint");
    assert!(matches!(load(&bad_hex), Err(BundleError::Malformed(_))));
    let mut bad_pitch = base;
    bad_pitch["source"]["tracks"][0]["tuning"][0] = json!(200);
    assert_eq!(
        load(&bad_pitch),
        Err(BundleError::Projection(
            griff_experiment::ProjectionError::InvalidPitch(200)
        ))
    );
    assert!(matches!(
        ExperimentBundleV1::from_json("{"),
        Err(BundleError::Malformed(_))
    ));
}

#[test]
fn a_spec_whose_recorded_identity_the_code_no_longer_has_is_not_rebuilt() {
    let mut drifted = bundle(&run());
    drifted.spec.variants[0].selector.identity.version = 2;
    assert!(matches!(
        drifted.spec(),
        Err(BundleError::IdentityDrift {
            stage: Stage::Selector,
            ..
        })
    ));
    let _ = InformationRegime::FULL;
}

// ── C4b: fail-closed writing, and no displayed fact outside an identity ──────

#[test]
fn a_non_finite_metric_is_refused_before_anything_is_written() {
    let mut written = bundle(&run());
    let CellOutcomeV1::Produced(result) = &mut written.cells[0].outcome else {
        panic!("produced");
    };
    result.metrics[0].value = f64::NAN;
    assert_eq!(
        written.to_json(),
        Err(BundleError::NonFiniteMetric { cell: 0, metric: 0 }),
        "never a `null` standing in for a number"
    );
}

#[test]
fn population_metadata_is_part_of_the_population_identity() {
    let base = json_of(&run());
    for (field, value) in [
        ("reference_count", json!(12)),
        ("rhythm_count", json!(0)),
        ("gesture_present", json!(false)),
    ] {
        let mut value_edit = base.clone();
        value_edit["population"][field] = value;
        assert_eq!(
            load(&value_edit),
            Err(BundleError::IdentityMismatch(Mismatch::Population)),
            "{field} is a displayed fact, bound to the snapshot"
        );
    }
}

#[test]
fn what_a_pass_claims_happened_is_bound_to_its_record() {
    let base = json_of(&run());
    let refused = |edit: &dyn Fn(&mut Value)| {
        let mut value = base.clone();
        edit(&mut value);
        load(&value).expect_err("refused")
    };
    for edit in [
        (&|v: &mut Value| v["passes"][1]["candidate_count"] = json!(3)) as &dyn Fn(&mut Value),
        &|v: &mut Value| v["passes"][1]["contribution"]["templates"] = json!(99),
        &|v: &mut Value| v["passes"][1]["contribution"]["references"] = json!(0),
        &|v: &mut Value| v["passes"][1]["contribution"]["gesture"] = json!(false),
        &|v: &mut Value| v["passes"][1]["candidates"] = base["passes"][0]["candidates"].clone(),
    ] {
        assert_eq!(
            refused(edit),
            BundleError::IdentityMismatch(Mismatch::PassRecord { pass: 1 })
        );
    }
}

#[test]
fn a_pass_regime_is_bound_even_when_no_population_makes_it_invisible_to_generation() {
    let source = source();
    let run = run_experiment(
        &spec(),
        &ExperimentInputs {
            source: &source,
            corpus: None,
        },
    )
    .expect("runs");
    let mut value: Value = serde_json::from_str(
        &ExperimentBundleV1::from_run(&spec(), &source, &run)
            .expect("this run")
            .to_json()
            .expect("serializes"),
    )
    .expect("valid JSON");
    // Without a population every regime is offered the same empty view, so
    // information cannot tell SEED_ONLY from FULL — the record must.
    value["passes"][0]["regime"]["references"] = json!(true);
    assert_eq!(
        load(&value),
        Err(BundleError::IdentityMismatch(Mismatch::PassRecord {
            pass: 0
        }))
    );
}

#[test]
fn what_a_cell_claims_is_bound_to_its_record() {
    let base = json_of(&run());
    let refused = |edit: &dyn Fn(&mut Value)| {
        let mut value = base.clone();
        edit(&mut value);
        load(&value).expect_err("refused")
    };
    assert_eq!(
        refused(&|v| v["cells"][0]["outcome"]["produced"]["metrics"][2]["value"] = json!(0.123)),
        BundleError::IdentityMismatch(Mismatch::CellRecord { cell: 0 }),
        "the number a delta is computed from"
    );
    assert_eq!(
        refused(
            &|v| v["cells"][0]["outcome"]["produced"]["metrics"][2]["owner"]["version"] = json!(9)
        ),
        BundleError::IdentityMismatch(Mismatch::CellRecord { cell: 0 })
    );
    assert_eq!(
        refused(
            &|v| v["cells"][2]["outcome"]["produced"]["diagnostics"][0]["chain_bar"]["rank"] =
                json!(5)
        ),
        BundleError::IdentityMismatch(Mismatch::CellRecord { cell: 2 })
    );

    let mut with_refusal = run();
    with_refusal.cells[3].outcome = CellOutcome::Refused(CellRefusal::Chain(ChainError::Path(
        PathError::NonFiniteLocal {
            state: StateId {
                layer: 2,
                ordinal: 7,
            },
            cost: f64::INFINITY,
        },
    )));
    let mut value = json_of(&with_refusal);
    value["cells"][3]["outcome"]["refused"]["chain"]["path"]["non_finite_local"]["state"]
        ["ordinal"] = json!(8);
    assert_eq!(
        load(&value),
        Err(BundleError::IdentityMismatch(Mismatch::CellRecord {
            cell: 3
        })),
        "a refusal's detail is a claim too"
    );
}

#[test]
fn variant_labels_are_bound_to_the_run_record() {
    let mut value = json_of(&run());
    value["spec"]["variants"][0]["label"] = json!("Obviously Better");
    assert_eq!(
        load(&value),
        Err(BundleError::IdentityMismatch(Mismatch::Run)),
        "labels stay out of the spec identity, but not out of the record"
    );
}
