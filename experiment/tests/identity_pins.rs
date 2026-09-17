// Characterization: the identities this crate cannot read from their owners,
// pinned to golden behaviour (ADR-0034, version ownership).
//
// Each golden sits in one assertion with the identity version it vouches for.
// A failing pin means observable behaviour changed: bump that identity's
// version (or the fingerprint domain version) and update the golden in the
// same edit. Updating the golden alone is the one wrong fix — it would let a
// new behaviour keep an old name.
#![allow(clippy::expect_used, clippy::panic, clippy::missing_assert_message)]

mod common;

use common::{ask, corpus, source, two_by_two};
use griff_core::candidate_chain::chain_weights_v1;
use griff_core::generation_input::generation_request_from_score;
use griff_core::rerank::{rerank_weights_v1, RERANK_AXIS_LABELS};
use griff_experiment::{
    ask_fingerprint, gesture_fingerprint, references_fingerprint, rhythms_fingerprint,
    run_experiment, score_fingerprint, CellOutcome, EvaluationContext, ExperimentInputs,
    ExperimentRun, Fingerprint, GeneratorPolicy, InformationRegime, MetricKind, PolicyIdentity,
    ScorerPolicy, SelectorPolicy, EVALUATOR_GENERATION_AXES,
};

const INTACT: usize = 0;

fn evaluation() -> EvaluationContext {
    EvaluationContext::GenerationAxes {
        pitch_material: generation_request_from_score(&source(), 42, 4)
            .expect("seeds")
            .pitch_material,
        references: vec![source()],
    }
}

fn run() -> ExperimentRun {
    let (source, corpus) = (source(), corpus());
    run_experiment(
        &two_by_two(evaluation()),
        &ExperimentInputs {
            source: &source,
            corpus: Some(&corpus),
        },
    )
    .expect("the fixture runs")
}

const fn identity(id: &'static str, version: u32) -> PolicyIdentity {
    PolicyIdentity { id, version }
}

// ── owned elsewhere: read, never copied ──────────────────────────────────────

#[test]
fn production_identities_with_an_owner_are_the_owners_values() {
    assert_eq!(
        ScorerPolicy::GenerationRerankV1.identity(),
        PolicyIdentity::of_weights(&rerank_weights_v1())
    );
    assert_eq!(
        SelectorPolicy::GlobalChainV1.identity(),
        PolicyIdentity::of_weights(&chain_weights_v1())
    );
}

// ── fingerprint domain v1 ────────────────────────────────────────────────────

#[test]
fn fingerprint_domain_v1_is_pinned() {
    let c = corpus();
    assert_eq!(
        [
            score_fingerprint(&source()).to_hex(),
            rhythms_fingerprint(&c.rhythms).to_hex(),
            references_fingerprint(&c.references).to_hex(),
            gesture_fingerprint(c.gesture).to_hex(),
            ask_fingerprint(&ask()).to_hex(),
        ],
        [
            "68dd14f037667fc707454825c5bb038c4dd3d27e76b88ef1857efb90fa9ea5a9",
            "c6250a9044bd6c14887f631f7f50b84a0c8d03f8c2aaa93f61030a85b766fff3",
            "de73d4a5425380c80891a5cb68dde5ed0d351c29a0b83a0380785f0e59082a7b",
            "358c01cd2a28f86e3bb667dce628aad4438a5a1607ea20e6ad7755e6b1d0a29b",
            "400eb056d98e0b3644c595e3fbce01b0b3290b381ed3bd9fd097ff3c12e1dd87",
        ],
        "griff.{{score,rhythms,references,gesture,ask}}.v1 changed: bump the domain version"
    );
}

// ── manual contracts (debt) ──────────────────────────────────────────────────

#[test]
fn the_candidate_set_generator_identity_is_pinned_to_its_behaviour() {
    let run = run();
    let candidates: Vec<String> = run.passes.iter().map(|p| p.candidates.to_hex()).collect();
    assert_eq!(
        (GeneratorPolicy::S6CandidateSet.identity(), candidates),
        (
            identity("s6_candidate_set", 1),
            vec![
                "6686918281e9ac1b9f72a5bda7a8d7cc5f6503e2584189b282d438f2f3266e2f".to_owned(),
                "1617ebeebea20166858c85b7de6349a640740fb40806d83605506354086f11c5".to_owned()
            ]
        ),
        "the S6 candidate set changed: bump s6_candidate_set with this golden"
    );
}

#[test]
fn the_intact_selection_identity_is_pinned_to_its_behaviour() {
    let run = run();
    let content = |regime| match &run.cell(INTACT, regime).expect("cell").outcome {
        CellOutcome::Produced(result) => result.content.to_hex(),
        CellOutcome::Refused(refusal) => panic!("refused: {refusal:?}"),
    };
    assert_eq!(
        (
            SelectorPolicy::IntactTop.identity(),
            [
                content(InformationRegime::SEED_ONLY),
                content(InformationRegime::FULL)
            ]
        ),
        (
            identity("intact_top", 1),
            [
                "cf475deaf1cbd96098feccb532f6b95ceb37165a39e91ca28649cec24827ca89".to_owned(),
                "81d9c524ad06563d320457b92776e0c62c9c66d5c8e7ce3041cc37361496c31b".to_owned()
            ]
        ),
        "the intact selection changed: bump intact_top with this golden"
    );
}

#[test]
fn the_generation_axes_evaluator_identity_is_pinned_to_its_behaviour() {
    let run = run();
    let CellOutcome::Produced(result) = &run
        .cell(INTACT, InformationRegime::FULL)
        .expect("cell")
        .outcome
    else {
        panic!("the fixture's cell is produced");
    };
    let bits: Vec<u64> = RERANK_AXIS_LABELS
        .iter()
        .map(|&label| {
            result
                .metric(MetricKind::Evaluation, label)
                .expect("measured")
                .value
                .to_bits()
        })
        .collect();
    assert_eq!(
        (
            EVALUATOR_GENERATION_AXES,
            run.evaluation.expect("supplied").to_hex(),
            bits
        ),
        (
            identity("generation_axes", 1),
            "d48184980e116f5cee549520b77bd694a828a29883d6eeffe1ce8778eb1bfbc5".to_owned(),
            [1.0_f64, 0.8, 0.75, 0.8, 1.0, 1.0]
                .map(f64::to_bits)
                .to_vec()
        ),
        "the generation-axes evaluator changed: bump generation_axes with this golden"
    );
}

// ── run identities ───────────────────────────────────────────────────────────

#[test]
fn run_identity_domains_are_pinned() {
    let run = run();
    let hex =
        |fps: Vec<Fingerprint>| -> Vec<String> { fps.iter().map(Fingerprint::to_hex).collect() };
    assert_eq!(
        (
            run.spec.to_hex(),
            hex(run.passes.iter().map(|p| p.information).collect()),
            hex(run.cells.iter().map(|c| c.requested).collect()),
            hex(run.cells.iter().map(|c| c.recipe).collect()),
            run.corpus.as_ref().expect("bound").whole.to_hex(),
        ),
        (
            "d777770f67afa8532114422b2b922c7348f3e8684b5e7cc33e94d3168f37d79c".to_owned(),
            vec!["51bed29516571573c2606d283bbd193f0b295f69fbc704f3025605f3d33d143c".to_owned(), "1b7f8597b401ed16bc8619b6161ad2c179feffb125040ae41b61aea2a1ce2b28".to_owned()],
            vec!["PIN_REQ".to_owned()],
            vec!["f812b82c594daea718d6046bbae00566e09bdefab10aa58505e44e89c856cf7d".to_owned(), "f03694ae19d57571eb292ec8caf6b945d150b3807d920c042a32f95d4aa86f5a".to_owned(), "ef61e7e0ec75ed83208d04ca0038090711392e25458577a3b26f97b751b7f72d".to_owned(), "331c0fef28f9a08113cb1bb80160ec6f8a927c4e8582ee4e70056861ce77fee4".to_owned()],
            "PIN_POPULATION".to_owned(),
        ),
        "griff.experiment.{{spec,pass,cell-request,cell}}.v1 / corpus-snapshot.v2 changed: bump the domain version"
    );
}

#[test]
fn record_identity_domains_v1_are_pinned() {
    let run = run();
    let hex =
        |fps: Vec<Fingerprint>| -> Vec<String> { fps.iter().map(Fingerprint::to_hex).collect() };
    assert_eq!(
        (
            hex(run.passes.iter().map(|p| p.record).collect()),
            hex(run.cells.iter().map(|c| c.record).collect()),
            run.record.to_hex(),
        ),
        (
            vec!["PIN_PASS_RECORD".to_owned()],
            vec!["PIN_CELL_RECORD".to_owned()],
            "PIN_RUN_RECORD".to_owned(),
        ),
        "griff.experiment.{{pass-record,cell-record,run-record}}.v1 changed: bump the domain version"
    );
}
