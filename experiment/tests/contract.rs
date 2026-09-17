// TDD red: the in-memory Generator Observatory contract (design note §8,
// milestone 1) — variant × information regime cells over one source, one ask
// and one bound corpus population, with separate identities and metric
// comparability.
#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::missing_assert_message,
    clippy::float_cmp
)]

mod common;

use common::{ask, corpus, corpus_with_other_references, score_of, source, template, two_by_two};
use griff_core::candidate_chain::plan_candidate_chain;
use griff_core::closure::closure_axes;
use griff_core::generation_input::{
    generation_request_from_score, ranked_candidates, CorpusContribution, CorpusMaterial,
};
use griff_core::novelty::{measure_novelty, novelty_axes};
use griff_core::rerank::{rerank_weights_v1, RERANK_AXIS_LABELS};
use griff_experiment::{
    delta, interaction, references_fingerprint, rhythms_fingerprint, run_experiment,
    score_fingerprint, CellOutcome, Comparison, Diagnostic, EvaluationContext, ExperimentInputs,
    ExperimentResult, ExperimentRun, ExperimentSpec, InformationRegime, MetricKind, RunError,
    ScorerPolicy, SpecError, Unavailable, VariantSpec, EVALUATOR_GENERATION_AXES, METRIC_AGGREGATE,
    METRIC_CHAIN_COST,
};

const INTACT: usize = 0;
const CHAIN: usize = 1;

fn run(spec: &ExperimentSpec, corpus: Option<&CorpusMaterial>) -> ExperimentRun {
    let source = source();
    run_experiment(
        spec,
        &ExperimentInputs {
            source: &source,
            corpus,
        },
    )
    .expect("the fixture runs")
}

/// An evaluation context supplied explicitly: the source's own scale and a
/// fixed reference set that is not the runtime population.
fn fixed_evaluation() -> EvaluationContext {
    EvaluationContext::GenerationAxes {
        pitch_material: generation_request_from_score(&source(), 42, 4)
            .expect("seeds")
            .pitch_material,
        references: vec![score_of(&[(0, 480, 45), (480, 480, 47), (960, 960, 50)])],
    }
}

fn result(run: &ExperimentRun, variant: usize, regime: InformationRegime) -> &ExperimentResult {
    match &run.cell(variant, regime).expect("the cell exists").outcome {
        CellOutcome::Produced(result) => result,
        CellOutcome::Refused(refusal) => panic!("the fixture's cell is refused: {refusal:?}"),
    }
}

// ── 1. deterministic replay ──────────────────────────────────────────────────

#[test]
fn the_same_spec_over_the_same_inputs_is_the_same_run() {
    let c = corpus();
    let spec = two_by_two(fixed_evaluation());
    assert_eq!(run(&spec, Some(&c)), run(&spec, Some(&c)));
}

#[test]
fn fingerprints_are_content_sensitive_and_order_sensitive() {
    let a = source();
    let mut b = source();
    assert_eq!(score_fingerprint(&a), score_fingerprint(&b));
    if let Some(griff_core::score::AtomEvent::Note(n)) =
        b.tracks[0].voices[0].event_groups[0].atoms.first_mut()
    {
        n.pitch = griff_core::event::Pitch(41);
    }
    assert_ne!(score_fingerprint(&a), score_fingerprint(&b), "one pitch");

    let x = template(&[(0, 960)]);
    let y = template(&[(0, 480), (480, 480)]);
    assert_ne!(
        rhythms_fingerprint(&[x.clone(), y.clone()]),
        rhythms_fingerprint(&[y, x]),
        "palette order is behaviour"
    );
    assert_ne!(
        references_fingerprint(&[]),
        references_fingerprint(std::slice::from_ref(&a)),
    );
}

// ── 2. existing S6 / S7 results are unchanged ────────────────────────────────

#[test]
fn every_cell_is_the_result_the_existing_path_produces() {
    let c = corpus();
    let run = run(&two_by_two(EvaluationContext::None), Some(&c));
    for (regime, material) in [
        (InformationRegime::SEED_ONLY, None),
        (InformationRegime::FULL, Some(&c)),
    ] {
        let set = ranked_candidates(&source(), material, &ask(), None).expect("seeds");
        assert_eq!(
            result(&run, INTACT, regime).score,
            set.ranked[0].value.score,
            "S6 Intact under {regime:?} is ranked candidate 0 of the existing pass"
        );
        assert_eq!(
            result(&run, CHAIN, regime).score,
            plan_candidate_chain(&set).expect("chain-compatible").score,
            "S7 Global Chain under {regime:?} is the existing plan over that set"
        );
    }
}

#[test]
fn the_declared_scorer_is_the_core_rerank_policy() {
    let policy = rerank_weights_v1();
    let identity = ScorerPolicy::GenerationRerankV1.identity();
    assert_eq!((identity.id, identity.version), (policy.id, policy.version));
}

// ── 3–4. one pass per information need, shared by the variants that can ─────

#[test]
fn cells_are_variant_by_regime_and_share_one_pass_per_regime() {
    let c = corpus();
    let run = run(&two_by_two(EvaluationContext::None), Some(&c));
    assert_eq!(run.cells.len(), 4);
    let order: Vec<(usize, InformationRegime)> = run
        .cells
        .iter()
        .map(|cell| (cell.variant, cell.regime))
        .collect();
    assert_eq!(
        order,
        [
            (INTACT, InformationRegime::SEED_ONLY),
            (INTACT, InformationRegime::FULL),
            (CHAIN, InformationRegime::SEED_ONLY),
            (CHAIN, InformationRegime::FULL),
        ]
    );
    assert_eq!(
        run.passes.len(),
        2,
        "two regimes, one generator and scorer: two passes, not four"
    );
    for regime in [InformationRegime::SEED_ONLY, InformationRegime::FULL] {
        let a = run.cell(INTACT, regime).expect("cell");
        let b = run.cell(CHAIN, regime).expect("cell");
        assert_eq!(a.pass, b.pass, "S6 and S7 read one ranked set");
        assert_eq!(run.passes[a.pass].regime, regime);
        assert_ne!(a.recipe, b.recipe, "they differ in the selector alone");
    }
    assert_ne!(run.passes[0].candidates, run.passes[1].candidates);
}

// ── 6. requested regime ≠ actual contribution ────────────────────────────────

#[test]
fn a_full_regime_over_a_populated_corpus_contributes_every_channel() {
    let c = corpus();
    let run = run(&two_by_two(EvaluationContext::None), Some(&c));
    let full = run.cell(INTACT, InformationRegime::FULL).expect("cell");
    assert_eq!(
        run.passes[full.pass].contribution,
        CorpusContribution {
            templates: 2,
            references: 2,
            gesture: true,
        }
    );
    let seed = run
        .cell(INTACT, InformationRegime::SEED_ONLY)
        .expect("cell");
    assert!(run.passes[seed.pass].contribution.is_seed_only());
}

#[test]
fn a_full_regime_without_a_corpus_is_requested_full_and_actually_seed_only() {
    let run = run(&two_by_two(EvaluationContext::None), None);
    let full = run.cell(INTACT, InformationRegime::FULL).expect("cell");
    assert_eq!(full.regime, InformationRegime::FULL, "what was asked");
    assert!(
        run.passes[full.pass].contribution.is_seed_only(),
        "what was taken"
    );
    assert!(run.corpus.is_none(), "no population was bound");
}

#[test]
fn a_full_regime_over_an_empty_population_is_actually_seed_only() {
    let empty = CorpusMaterial {
        rhythms: Vec::new(),
        references: Vec::new(),
        gesture: None,
        skipped: vec!["every record skipped".to_owned()],
    };
    let run = run(&two_by_two(EvaluationContext::None), Some(&empty));
    let full = run.cell(INTACT, InformationRegime::FULL).expect("cell");
    assert!(run.passes[full.pass].contribution.is_seed_only());
    assert_eq!(
        run.corpus.as_ref().expect("a population was bound").skipped,
        ["every record skipped"]
    );
}

#[test]
fn each_single_channel_regime_contributes_only_its_channel() {
    let c = corpus();
    let spec = ExperimentSpec {
        regimes: InformationRegime::all().to_vec(),
        ..two_by_two(EvaluationContext::None)
    };
    let run = run(&spec, Some(&c));
    let taken = |regime| {
        let cell = run.cell(INTACT, regime).expect("cell");
        run.passes[cell.pass].contribution
    };
    let only = |templates, references, gesture| CorpusContribution {
        templates,
        references,
        gesture,
    };
    assert_eq!(taken(InformationRegime::RHYTHMS_ONLY), only(2, 0, false));
    assert_eq!(taken(InformationRegime::REFERENCES_ONLY), only(0, 2, false));
    assert_eq!(taken(InformationRegime::GESTURE_ONLY), only(0, 0, true));
}

#[test]
fn every_regime_combination_runs_without_a_special_case() {
    let all = InformationRegime::all();
    assert_eq!(all[0], InformationRegime::SEED_ONLY);
    assert_eq!(all[7], InformationRegime::FULL);
    let mut distinct = all.to_vec();
    distinct.sort();
    distinct.dedup();
    assert_eq!(distinct.len(), 8);
}

// ── separate identities ──────────────────────────────────────────────────────

#[test]
fn a_cell_identity_depends_only_on_the_channels_it_could_consume() {
    let spec = ExperimentSpec {
        regimes: InformationRegime::all().to_vec(),
        ..two_by_two(EvaluationContext::None)
    };
    let (c, changed) = (corpus(), corpus_with_other_references());
    let (before, after) = (run(&spec, Some(&c)), run(&spec, Some(&changed)));
    let recipe = |run: &ExperimentRun, regime| run.cell(INTACT, regime).expect("cell").recipe;

    for regime in InformationRegime::all() {
        if regime.references {
            assert_ne!(
                recipe(&before, regime),
                recipe(&after, regime),
                "{regime:?} consumed the changed references"
            );
        } else {
            assert_eq!(
                recipe(&before, regime),
                recipe(&after, regime),
                "{regime:?} never saw the references"
            );
        }
    }

    let (s0, s1) = (
        before.corpus.as_ref().expect("bound"),
        after.corpus.as_ref().expect("bound"),
    );
    assert_ne!(s0.whole, s1.whole, "the bound population did change");
    assert_ne!(s0.references, s1.references);
    assert_eq!(s0.rhythms, s1.rhythms);
    assert_eq!(s0.gesture, s1.gesture);
}

#[test]
fn a_seed_only_cell_is_the_same_recipe_with_or_without_a_population() {
    let c = corpus();
    let spec = two_by_two(EvaluationContext::None);
    let with = run(&spec, Some(&c));
    let without = run(&spec, None);
    let recipe = |run: &ExperimentRun| {
        run.cell(INTACT, InformationRegime::SEED_ONLY)
            .expect("cell")
            .recipe
    };
    assert_eq!(recipe(&with), recipe(&without));
    assert_eq!(
        with.cell(INTACT, InformationRegime::SEED_ONLY)
            .expect("cell")
            .outcome,
        without
            .cell(INTACT, InformationRegime::SEED_ONLY)
            .expect("cell")
            .outcome,
    );
}

#[test]
fn the_spec_population_and_evaluation_identities_are_separate() {
    let c = corpus();
    let none = run(&two_by_two(EvaluationContext::None), Some(&c));
    let fixed = run(&two_by_two(fixed_evaluation()), Some(&c));
    assert_eq!(none.evaluation, None);
    assert!(fixed.evaluation.is_some());
    assert_ne!(
        none.spec, fixed.spec,
        "the evaluation context is part of the spec"
    );
    assert_eq!(none.corpus, fixed.corpus, "the population is not");
    assert_eq!(none.passes, fixed.passes, "nor is what a pass consumed");
    assert_eq!(
        none.cells.iter().map(|c| c.recipe).collect::<Vec<_>>(),
        fixed.cells.iter().map(|c| c.recipe).collect::<Vec<_>>(),
        "nor what produced a cell"
    );
}

// ── metric comparability ─────────────────────────────────────────────────────

#[test]
fn without_an_evaluator_there_is_no_evaluation_and_no_interaction() {
    let c = corpus();
    let run = run(&two_by_two(EvaluationContext::None), Some(&c));
    let axis = RERANK_AXIS_LABELS[0];
    let get = |variant, regime| result(&run, variant, regime).metric(MetricKind::Evaluation, axis);
    assert!(get(INTACT, InformationRegime::FULL).is_none());
    assert_eq!(
        interaction(
            get(INTACT, InformationRegime::SEED_ONLY),
            get(CHAIN, InformationRegime::SEED_ONLY),
            get(INTACT, InformationRegime::FULL),
            get(CHAIN, InformationRegime::FULL),
        ),
        Comparison::Unavailable(Unavailable::Missing)
    );
}

#[test]
fn a_fixed_evaluator_measures_every_cell_on_one_scale() {
    let c = corpus();
    let evaluation = fixed_evaluation();
    let run = run(&two_by_two(evaluation.clone()), Some(&c));
    let EvaluationContext::GenerationAxes {
        pitch_material,
        references,
    } = &evaluation
    else {
        panic!("fixture");
    };

    for label in RERANK_AXIS_LABELS {
        let get =
            |variant, regime| result(&run, variant, regime).metric(MetricKind::Evaluation, label);
        let cells = [
            get(INTACT, InformationRegime::SEED_ONLY),
            get(CHAIN, InformationRegime::SEED_ONLY),
            get(INTACT, InformationRegime::FULL),
            get(CHAIN, InformationRegime::FULL),
        ];
        let values: Vec<_> = cells.iter().map(|m| m.expect("measured")).collect();
        for m in &values {
            assert_eq!(m.identity, values[0].identity, "{label}: one identity");
            assert_eq!(m.identity.owner, EVALUATOR_GENERATION_AXES);
            assert_eq!(Some(m.identity.context), run.evaluation);
        }
        let [a0, b0, a1, b1] = [
            values[0].value,
            values[1].value,
            values[2].value,
            values[3].value,
        ];
        assert_eq!(
            interaction(cells[0], cells[1], cells[2], cells[3]),
            Comparison::Available((b1 - a1) - (b0 - a0)),
            "{label}"
        );
        assert_eq!(
            delta(cells[0], cells[2]),
            Comparison::Available(a1 - a0),
            "{label}: across regimes, on one scale"
        );
    }

    // The evaluator is the published measurement, not a private re-derivation.
    let intact = result(&run, INTACT, InformationRegime::FULL);
    let closure = closure_axes(&intact.score, 0, pitch_material).expect("measures");
    let novelty = novelty_axes(&measure_novelty(&intact.score, 0, references).expect("measures"));
    for axis in closure.iter().chain(novelty.iter()) {
        assert_eq!(
            intact
                .metric(MetricKind::Evaluation, axis.label)
                .map(|m| m.value),
            Some(axis.value),
            "{}",
            axis.label
        );
    }
}

#[test]
fn a_policy_objective_compares_within_its_pass_and_nowhere_else() {
    let c = corpus();
    let run = run(&two_by_two(fixed_evaluation()), Some(&c));
    let cost = |variant, regime| {
        result(&run, variant, regime).metric(MetricKind::PolicyObjective, METRIC_CHAIN_COST)
    };

    let set = ranked_candidates(&source(), Some(&c), &ask(), None).expect("seeds");
    let plan = plan_candidate_chain(&set).expect("chain-compatible");
    let intact_cost = griff_core::candidate_chain::intact_s6_cost(&set).expect("compatible");
    assert_eq!(
        delta(
            cost(INTACT, InformationRegime::FULL),
            cost(CHAIN, InformationRegime::FULL)
        ),
        Comparison::Available(plan.total_cost - intact_cost),
        "S6 vs S7 under candidate_chain v1, over one ranked set"
    );
    assert_eq!(
        delta(
            cost(INTACT, InformationRegime::SEED_ONLY),
            cost(INTACT, InformationRegime::FULL)
        ),
        Comparison::Unavailable(Unavailable::IncompatibleIdentity),
        "the chain cost scale moves with the references the pass saw"
    );
    assert_eq!(
        interaction(
            cost(INTACT, InformationRegime::SEED_ONLY),
            cost(CHAIN, InformationRegime::SEED_ONLY),
            cost(INTACT, InformationRegime::FULL),
            cost(CHAIN, InformationRegime::FULL),
        ),
        Comparison::Unavailable(Unavailable::NotAnEvaluation)
    );
    assert_eq!(
        delta(
            result(&run, INTACT, InformationRegime::FULL)
                .metric(MetricKind::PolicyObjective, METRIC_AGGREGATE),
            cost(CHAIN, InformationRegime::FULL)
        ),
        Comparison::Unavailable(Unavailable::IncompatibleIdentity),
        "a rerank aggregate and a chain cost are two scales"
    );
    assert_eq!(
        result(&run, CHAIN, InformationRegime::FULL)
            .metric(MetricKind::PolicyObjective, METRIC_AGGREGATE),
        None,
        "a chain has no single rerank aggregate"
    );
}

// ── results, realization, diagnostics ────────────────────────────────────────

#[test]
fn no_current_policy_fabricates_a_realization() {
    let c = corpus();
    let run = run(&two_by_two(EvaluationContext::None), Some(&c));
    for cell in &run.cells {
        let CellOutcome::Produced(result) = &cell.outcome else {
            panic!("the fixture's cells are produced");
        };
        assert!(result.realization.is_none());
        assert_eq!(result.content, score_fingerprint(&result.score));
    }
}

#[test]
fn diagnostics_name_the_candidates_a_result_was_selected_from() {
    let c = corpus();
    let run = run(&two_by_two(EvaluationContext::None), Some(&c));
    let pass = &run.passes[run
        .cell(INTACT, InformationRegime::FULL)
        .expect("cell")
        .pass];

    let intact = result(&run, INTACT, InformationRegime::FULL);
    assert!(matches!(
        intact.diagnostics.as_slice(),
        [Diagnostic::Selected {
            candidate: 0,
            rank: 1,
            ..
        }]
    ));

    let chain = result(&run, CHAIN, InformationRegime::FULL);
    assert_eq!(chain.diagnostics.len(), ask().bars, "one supplier per bar");
    for (i, d) in chain.diagnostics.iter().enumerate() {
        let Diagnostic::ChainBar {
            bar,
            candidate,
            rank,
            ..
        } = *d
        else {
            panic!("a chain result is described bar by bar");
        };
        assert_eq!(bar, i);
        assert!(candidate < pass.candidate_count);
        assert_eq!(rank, candidate + 1);
    }
}

// ── spec validation ──────────────────────────────────────────────────────────

fn refused(spec: &ExperimentSpec) -> SpecError {
    let source = source();
    match run_experiment(
        spec,
        &ExperimentInputs {
            source: &source,
            corpus: None,
        },
    ) {
        Err(RunError::Spec(error)) => error,
        other => panic!("expected a spec refusal, got {other:?}"),
    }
}

#[test]
fn a_regime_whose_gesture_the_ask_declines_is_refused_not_run() {
    let spec = ExperimentSpec {
        ask: griff_core::generation_input::GenerationAsk {
            gesture: false,
            ..ask()
        },
        ..two_by_two(EvaluationContext::None)
    };
    assert_eq!(
        refused(&spec),
        SpecError::GestureChannelDeclinedByAsk(InformationRegime::FULL)
    );
}

#[test]
fn an_empty_or_duplicated_axis_is_refused() {
    let base = two_by_two(EvaluationContext::None);
    assert_eq!(
        refused(&ExperimentSpec {
            variants: Vec::new(),
            ..base.clone()
        }),
        SpecError::NoVariants
    );
    assert_eq!(
        refused(&ExperimentSpec {
            regimes: Vec::new(),
            ..base.clone()
        }),
        SpecError::NoRegimes
    );
    assert_eq!(
        refused(&ExperimentSpec {
            regimes: vec![InformationRegime::FULL, InformationRegime::FULL],
            ..base.clone()
        }),
        SpecError::DuplicateRegime(InformationRegime::FULL)
    );
    assert_eq!(
        refused(&ExperimentSpec {
            variants: vec![VariantSpec::s6_intact(), VariantSpec::s6_intact()],
            ..base
        }),
        SpecError::DuplicateVariantLabel("S6 Intact".to_owned())
    );
}
