// TDD red: the Observatory view is the bundle arranged for display, and every
// number it compares comes from the experiment API.
#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::missing_assert_message,
    clippy::float_cmp
)]

use griff_core::event::{NoteMarks, Pitch, Tempo, Ticks, TimeSignature, Tuning, Velocity};
use griff_core::generate::{RhythmTemplate, TemplateNote};
use griff_core::generation_input::{generation_request_from_score, CorpusMaterial, GenerationAsk};
use griff_core::gesture::GestureControl;
use griff_core::score::{
    index_from_ordinal, AtomEvent, AtomNote, EventGroup, EventGroupKind, LossReport, MasterBar,
    RepeatMarker, Score, Track, Voice,
};
use griff_core::slice::TickRange;
use griff_experiment::{
    delta, interaction, run_experiment, CellOutcome, CellOutcomeV1, CellRefusal, CellRefusalV1,
    Comparison, EvaluationContext, ExperimentBundleV1, ExperimentInputs, ExperimentRun,
    ExperimentSpec, InformationRegime, MetricKind, Unavailable, VariantSpec, METRIC_AGGREGATE,
    METRIC_CHAIN_COST,
};

use super::{CellOutcomeView, EvaluationView, ExperimentView, RegimeName, StageKind};

fn score_of(notes: &[(u32, u32, u8)]) -> Score {
    let master_bars = (0..2_u32)
        .map(|i| {
            let start = i.saturating_mul(1920);
            MasterBar {
                index: index_from_ordinal(usize::try_from(i).expect("small")),
                tick_range: TickRange::new(Ticks(start), Ticks(start.saturating_add(1920)))
                    .expect("ordered"),
                time_signature: TimeSignature::new(4, 4).expect("4/4"),
                tempo: Tempo::from_bpm_integer(120).expect("120 BPM"),
                repeat: RepeatMarker::default(),
            }
        })
        .collect();
    Score {
        ticks_per_quarter: 480,
        master_bars,
        tracks: vec![Track {
            name: Some("guitar".to_owned()),
            channel: 0,
            voices: vec![Voice {
                id: 0,
                event_groups: notes
                    .iter()
                    .map(|&(onset, duration, pitch)| EventGroup {
                        kind: EventGroupKind::Single,
                        atoms: vec![AtomEvent::Note(AtomNote {
                            absolute_start: Ticks(onset),
                            duration: Ticks(duration),
                            pitch: Pitch::new(pitch).expect("pitch"),
                            velocity: Velocity::new(96).expect("velocity"),
                            marks: NoteMarks::empty(),
                            position: None,
                        })],
                        technique_spans: Vec::new(),
                    })
                    .collect(),
            }],
            tuning: Tuning::standard_e(),
        }],
        source_meta: None,
        loss: LossReport::new(),
    }
}

fn source() -> Score {
    score_of(&[
        (0, 480, 40),
        (480, 480, 43),
        (960, 480, 45),
        (1440, 480, 47),
        (1920, 960, 50),
        (2880, 480, 47),
        (3360, 480, 45),
    ])
}

fn corpus() -> CorpusMaterial {
    let template = |notes: &[(u32, u32)]| RhythmTemplate {
        notes: notes
            .iter()
            .map(|&(offset, duration)| TemplateNote {
                offset: Ticks(offset),
                duration: Ticks(duration),
            })
            .collect(),
    };
    CorpusMaterial {
        rhythms: vec![
            template(&[(0, 240), (240, 240), (480, 480), (960, 960)]),
            template(&[(0, 480), (720, 240), (960, 240), (1440, 480)]),
        ],
        references: vec![score_of(&[(0, 480, 40), (480, 480, 43), (960, 480, 45)])],
        gesture: Some(GestureControl {
            burst_notes: 3,
            rest_quarters: 1.0,
        }),
        skipped: vec!["unreadable.chunk.json".to_owned()],
    }
}

fn spec() -> ExperimentSpec {
    ExperimentSpec {
        ask: GenerationAsk {
            seed: 42,
            bars: 4,
            variants_per_strategy: 2,
            gesture: true,
            tonal: None,
        },
        variants: vec![VariantSpec::s6_intact(), VariantSpec::s7_global_chain()],
        regimes: vec![InformationRegime::SEED_ONLY, InformationRegime::FULL],
        evaluation: EvaluationContext::GenerationAxes {
            pitch_material: generation_request_from_score(&source(), 42, 4)
                .expect("seeds")
                .pitch_material,
            references: vec![source()],
        },
    }
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
    .expect("runs")
}

fn bundle() -> ExperimentBundleV1 {
    ExperimentBundleV1::from_run(&spec(), &source(), &run()).expect("this run")
}

fn view() -> ExperimentView {
    ExperimentView::from_bundle(&bundle()).expect("projects")
}

// ── one display path ─────────────────────────────────────────────────────────

#[test]
fn a_fresh_bundle_and_the_same_bundle_loaded_project_to_one_view() {
    let fresh = bundle();
    let loaded =
        ExperimentBundleV1::from_json(&fresh.to_json().expect("serializes")).expect("loads");
    assert_eq!(
        ExperimentView::from_bundle(&fresh),
        ExperimentView::from_bundle(&loaded),
        "run → bundle → view and saved bundle → load → view are one path"
    );
}

#[test]
fn the_view_is_the_bundle_arranged() {
    let (bundle, run, view) = (bundle(), run(), view());
    assert_eq!(view.record, bundle.identities.record);
    assert_eq!(view.spec, bundle.identities.spec);
    assert_eq!(view.source, bundle.identities.source);
    assert_eq!(
        view.variants
            .iter()
            .map(|v| v.label.as_str())
            .collect::<Vec<_>>(),
        ["S6 Intact", "S7 Global Chain"]
    );
    assert_eq!(
        view.variants[1]
            .stages
            .iter()
            .map(|s| (s.stage, s.id.as_str(), s.version))
            .collect::<Vec<_>>(),
        [
            (StageKind::Generator, "s6_candidate_set", 1),
            (StageKind::Scorer, "generation_rerank", 1),
            (StageKind::Selector, "candidate_chain", 1),
            (StageKind::Realizer, "no_realization", 1),
        ]
    );
    assert_eq!(
        view.regimes.iter().map(|r| r.name).collect::<Vec<_>>(),
        [RegimeName::SeedOnly, RegimeName::Full]
    );
    let population = view.population.as_ref().expect("bound");
    assert_eq!(
        (
            population.rhythm_count,
            population.reference_count,
            population.gesture_present,
            population.skipped
        ),
        (2, 1, true, 1)
    );
    assert!(matches!(
        view.evaluation,
        EvaluationView::GenerationAxes { references: 1, .. }
    ));

    assert_eq!(view.cells.len(), run.cells.len());
    for (shown, recorded) in view.cells.iter().zip(&run.cells) {
        assert_eq!(
            (shown.variant, view.regimes[shown.regime].channels),
            (recorded.variant, recorded.regime)
        );
        assert_eq!(shown.requested, recorded.requested);
        assert_eq!(shown.effective.recipe, recorded.recipe);
        let pass = &run.passes[recorded.pass];
        assert_eq!(shown.effective.information, pass.information);
        assert_eq!(shown.effective.contribution, pass.contribution);
        assert_eq!(shown.effective.candidate_count, pass.candidate_count);
        let (CellOutcomeView::Produced { score, metrics, .. }, CellOutcome::Produced(result)) =
            (&shown.outcome, &recorded.outcome)
        else {
            panic!("the fixture's cells are produced");
        };
        assert_eq!(score, &result.score, "auditioning plays the recorded score");
        assert_eq!(metrics, &result.metrics);
    }
}

#[test]
fn requested_effective_and_actual_stay_apart() {
    let view = view();
    let seed = &view.cells[view.cell_index(0, 0).expect("cell")];
    let full = &view.cells[view.cell_index(0, 1).expect("cell")];
    assert_ne!(seed.requested, full.requested);
    assert!(seed.effective.contribution.is_seed_only());
    assert_eq!(full.effective.contribution.templates, 2);
    assert_eq!(full.effective.contribution.references, 1);
    assert!(full.effective.contribution.gesture);
}

#[test]
fn presets_are_named_and_anything_else_is_custom() {
    assert_eq!(
        RegimeName::of(InformationRegime::SEED_ONLY),
        RegimeName::SeedOnly
    );
    assert_eq!(
        RegimeName::of(InformationRegime::RHYTHMS_ONLY),
        RegimeName::RhythmsOnly
    );
    assert_eq!(
        RegimeName::of(InformationRegime::REFERENCES_ONLY),
        RegimeName::ReferencesOnly
    );
    assert_eq!(
        RegimeName::of(InformationRegime::GESTURE_ONLY),
        RegimeName::GestureOnly
    );
    assert_eq!(RegimeName::of(InformationRegime::FULL), RegimeName::Full);
    assert_eq!(
        RegimeName::of(InformationRegime {
            rhythms: true,
            references: true,
            gesture: false
        }),
        RegimeName::Custom
    );
}

// ── no arithmetic of its own ─────────────────────────────────────────────────

fn metric<'a>(
    view: &'a ExperimentView,
    cell: usize,
    kind: MetricKind,
    name: &str,
) -> Option<&'a griff_experiment::MetricValue> {
    match &view.cells[cell].outcome {
        CellOutcomeView::Produced { metrics, .. } => metrics
            .iter()
            .find(|m| m.identity.kind == kind && m.identity.name == name),
        CellOutcomeView::Refused(_) => None,
    }
}

#[test]
fn an_ab_comparison_is_the_experiment_apis_delta_metric_by_metric() {
    let view = view();
    let a = view.cell_index(0, 0).expect("S6 / seed only");
    let b = view.cell_index(1, 1).expect("S7 / full");
    let rows = view.compare(a, b);
    assert!(!rows.is_empty());
    for row in &rows {
        let (ma, mb) = (
            metric(&view, a, row.kind, row.name),
            metric(&view, b, row.kind, row.name),
        );
        assert_eq!(row.a, ma.map(|m| m.value));
        assert_eq!(row.b, mb.map(|m| m.value));
        assert_eq!(row.comparison, delta(ma, mb), "{}", row.name);
    }
    let cost = rows
        .iter()
        .find(|r| r.kind == MetricKind::PolicyObjective && r.name == METRIC_CHAIN_COST)
        .expect("both cells weigh a chain cost");
    assert_eq!(
        cost.comparison,
        Comparison::Unavailable(Unavailable::IncompatibleIdentity),
        "two regimes, two scales: no number"
    );
    let aggregate = rows
        .iter()
        .find(|r| r.name == METRIC_AGGREGATE)
        .expect("A has an aggregate");
    assert_eq!(
        (aggregate.b, aggregate.comparison),
        (None, Comparison::Unavailable(Unavailable::Missing))
    );
    assert!(rows
        .iter()
        .filter(|r| r.kind == MetricKind::Evaluation)
        .all(|r| matches!(r.comparison, Comparison::Available(_))));
}

#[test]
fn a_same_regime_comparison_has_its_policy_objective_delta() {
    let view = view();
    let rows = view.compare(
        view.cell_index(0, 1).expect("S6 / full"),
        view.cell_index(1, 1).expect("S7 / full"),
    );
    let cost = rows
        .iter()
        .find(|r| r.kind == MetricKind::PolicyObjective && r.name == METRIC_CHAIN_COST)
        .expect("chain cost");
    assert!(matches!(cost.comparison, Comparison::Available(_)));
}

#[test]
fn an_interaction_is_the_experiment_apis_for_evaluations_only() {
    let view = view();
    let rows = view.interaction((0, 1), (0, 1));
    assert_eq!(rows.len(), 6, "the six evaluation axes, no objectives");
    for row in &rows {
        let get = |variant, regime| {
            metric(
                &view,
                view.cell_index(variant, regime).expect("cell"),
                MetricKind::Evaluation,
                row.name,
            )
        };
        assert_eq!(
            row.comparison,
            interaction(get(0, 0), get(1, 0), get(0, 1), get(1, 1)),
            "{}",
            row.name
        );
    }
}

// ── a refusal is shown as a refusal ──────────────────────────────────────────

#[test]
fn a_refused_cell_projects_as_its_refusal_with_nothing_to_play() {
    let mut bundle = bundle();
    bundle.cells[3].outcome = CellOutcomeV1::Refused(CellRefusalV1::EmptySet);
    let view = ExperimentView::from_bundle(&bundle).expect("projects");
    assert_eq!(
        view.cells[3].outcome,
        CellOutcomeView::Refused(CellRefusal::EmptySet)
    );
    assert!(view
        .compare(0, 3)
        .iter()
        .all(|r| r.b.is_none() && r.comparison == Comparison::Unavailable(Unavailable::Missing)));
}
