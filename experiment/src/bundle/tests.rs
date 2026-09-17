//! Bundle properties that need a *consistent* forgery: a run changed after it
//! was made and resealed through the crate's one sealing path. Outside the
//! crate a changed run cannot be sealed, so these live here.
#![allow(
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::missing_assert_message
)]

use griff_core::candidate_chain::ChainError;
use griff_core::event::{NoteMarks, Pitch, Tempo, Ticks, TimeSignature, Tuning, Velocity};
use griff_core::generate::{RhythmTemplate, TemplateNote};
use griff_core::generation_input::{generation_request_from_score, CorpusMaterial, GenerationAsk};
use griff_core::gesture::GestureControl;
use griff_core::layered_path::{PathError, StateId};
use griff_core::score::{
    index_from_ordinal, AtomEvent, AtomNote, EventGroup, EventGroupKind, LossReport, MasterBar,
    RepeatMarker, Score, Track, Voice,
};
use griff_core::slice::TickRange;
use serde_json::{json, Value};

use super::{BundleError, ExperimentBundleV1, Mismatch};
use crate::fingerprint::score_fingerprint;
use crate::identity;
use crate::regime::InformationRegime;
use crate::run::{run_experiment, CellOutcome, CellRefusal, ExperimentInputs, ExperimentRun};
use crate::spec::{EvaluationContext, ExperimentSpec, VariantSpec};

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
    let event_groups = notes
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
        .collect();
    Score {
        ticks_per_quarter: 480,
        master_bars,
        tracks: vec![Track {
            name: Some("guitar".to_owned()),
            channel: 0,
            voices: vec![Voice {
                id: 0,
                event_groups,
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
        skipped: Vec::new(),
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
            references: vec![score_of(&[(0, 960, 45), (960, 960, 50)])],
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

/// Reseals a changed run through the one sealing path the runner uses.
fn sealed(mut run: ExperimentRun) -> ExperimentRun {
    let spec = spec();
    let labels: Vec<&str> = spec.variants.iter().map(|v| v.label.as_str()).collect();
    identity::seal(&mut run, &labels);
    run
}

fn round_trip(run: &ExperimentRun) -> ExperimentBundleV1 {
    let json = ExperimentBundleV1::from_run(&spec(), &source(), run)
        .expect("a sealed run is written")
        .to_json()
        .expect("serializes");
    ExperimentBundleV1::from_json(&json).expect("loads")
}

#[test]
fn sealing_an_unchanged_run_changes_nothing() {
    let run = run();
    assert_eq!(
        sealed(run.clone()),
        run,
        "the runner seals through the same path"
    );
}

// ── gate 1: loading never generates ──────────────────────────────────────────

#[test]
fn loading_shows_the_recorded_score_not_a_regenerated_one() {
    let generated = run();
    let mut forged = generated.clone();
    let foreign = score_of(&[(0, 1920, 64)]);
    let CellOutcome::Produced(result) = &mut forged.cells[0].outcome else {
        panic!("produced");
    };
    result.score = foreign.clone();
    result.content = score_fingerprint(&foreign);
    let forged = sealed(forged);

    let loaded = round_trip(&forged).run().expect("rebuilds");
    let (CellOutcome::Produced(shown), CellOutcome::Produced(original)) =
        (&loaded.cells[0].outcome, &generated.cells[0].outcome)
    else {
        panic!("produced");
    };
    assert_eq!(shown.score, foreign, "shown as recorded");
    assert_ne!(
        shown.score, original.score,
        "and not as the generator would"
    );
}

// ── gate 3: lossless, refusals included ──────────────────────────────────────

fn with_refusals() -> ExperimentRun {
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
    sealed(run)
}

#[test]
fn a_refused_cell_survives_its_bundle_exactly() {
    let run = with_refusals();
    assert_eq!(
        round_trip(&run).run(),
        Ok(run),
        "a refusal stays typed, cost bits included"
    );
}

#[test]
fn a_refusals_detail_is_bound_to_the_cell_record() {
    let bundle =
        ExperimentBundleV1::from_run(&spec(), &source(), &with_refusals()).expect("sealed");
    let mut value: Value =
        serde_json::from_str(&bundle.to_json().expect("serializes")).expect("valid JSON");
    value["cells"][3]["outcome"]["refused"]["chain"]["path"]["non_finite_local"]["state"]
        ["ordinal"] = json!(8);
    assert_eq!(
        ExperimentBundleV1::from_json(&value.to_string()),
        Err(BundleError::IdentityMismatch(Mismatch::CellRecord {
            cell: 3
        }))
    );
}
