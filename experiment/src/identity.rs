//! The run identities, defined once.
//!
//! The runner computes these while it runs; a loaded bundle recomputes them
//! from what it records. Both call the functions here, over the canonical
//! projection, so a run's identity and its bundle's verification cannot
//! disagree about what an identity is (ADR-0034 decisions 4 and 5).

use crate::bundle::{
    CellOutcomeV1, CellRefusalV1, CellV1, CorpusContributionV1, DiagnosticV1, EvaluationContextV1,
    ExperimentSpecV1, GenerationPassV1, MetricValueV1, PolicyIdentityV1,
};
use crate::fingerprint::{
    gesture_fingerprint, references_fingerprint, references_fingerprint_of, rhythms_fingerprint,
    Fingerprint, Hasher,
};
use crate::projection::ScoreV1;
use crate::regime::InformationRegime;
use crate::run::{Cell, CellOutcome, CorpusSnapshot, ExperimentRun, GenerationPass};
use crate::spec::PolicyIdentity;

/// A policy identity by reference — the runtime's static one or a recorded
/// one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PolicyRef<'a> {
    pub(crate) id: &'a str,
    pub(crate) version: u32,
}

impl From<PolicyIdentity> for PolicyRef<'static> {
    fn from(identity: PolicyIdentity) -> Self {
        Self {
            id: identity.id,
            version: identity.version,
        }
    }
}

impl<'a> From<&'a PolicyIdentityV1> for PolicyRef<'a> {
    fn from(identity: &'a PolicyIdentityV1) -> Self {
        Self {
            id: &identity.id,
            version: identity.version,
        }
    }
}

/// A variant's four stage identities.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Stages<'a> {
    pub(crate) generator: PolicyRef<'a>,
    pub(crate) scorer: PolicyRef<'a>,
    pub(crate) selector: PolicyRef<'a>,
    pub(crate) realizer: PolicyRef<'a>,
}

/// One fingerprint per corpus channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Channels {
    pub(crate) rhythms: Fingerprint,
    pub(crate) references: Fingerprint,
    pub(crate) gesture: Fingerprint,
}

impl Channels {
    /// The channels of a bound population.
    pub(crate) const fn of(snapshot: &CorpusSnapshot) -> Self {
        Self {
            rhythms: snapshot.rhythms,
            references: snapshot.references,
            gesture: snapshot.gesture,
        }
    }

    /// Every channel empty.
    fn empty() -> Self {
        Self {
            rhythms: rhythms_fingerprint(&[]),
            references: references_fingerprint(&[]),
            gesture: gesture_fingerprint(None),
        }
    }
}

fn write_policy(h: &mut Hasher, policy: PolicyRef<'_>) {
    h.str(policy.id);
    h.u32(policy.version);
}

/// The channels `regime` offers from `population`: each open channel whole,
/// each masked or absent channel empty.
pub(crate) fn offered(regime: InformationRegime, population: Option<Channels>) -> Channels {
    let empty = Channels::empty();
    let Some(population) = population else {
        return empty;
    };
    Channels {
        rhythms: if regime.rhythms {
            population.rhythms
        } else {
            empty.rhythms
        },
        references: if regime.references {
            population.references
        } else {
            empty.references
        },
        gesture: if regime.gesture {
            population.gesture
        } else {
            empty.gesture
        },
    }
}

/// A count as recorded: `u64` on the wire and in every identity.
pub(crate) fn wide(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

/// What a bound population's snapshot records besides its channels — facts a
/// viewer displays, so facts its identity binds.
#[derive(Debug, Clone, Copy)]
pub(crate) struct SnapshotFacts<'a> {
    pub(crate) rhythm_count: u64,
    pub(crate) reference_count: u64,
    pub(crate) gesture_present: bool,
    pub(crate) skipped: &'a [String],
}

/// The whole identity of a bound population (v2: counts and gesture presence
/// joined the channels and the skipped records).
pub(crate) fn snapshot_whole(channels: Channels, facts: SnapshotFacts<'_>) -> Fingerprint {
    let mut h = Hasher::new("griff.experiment.corpus-snapshot.v2");
    h.fingerprint(channels.rhythms);
    h.fingerprint(channels.references);
    h.fingerprint(channels.gesture);
    h.u64(facts.rhythm_count);
    h.u64(facts.reference_count);
    h.bool(facts.gesture_present);
    h.usize(facts.skipped.len());
    for name in facts.skipped {
        h.str(name);
    }
    h.finish()
}

/// What an executed pass claims happened.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PassClaims {
    pub(crate) regime: InformationRegime,
    pub(crate) information: Fingerprint,
    pub(crate) contribution: CorpusContributionV1,
    pub(crate) candidates: Fingerprint,
    pub(crate) candidate_count: u64,
}

impl PassClaims {
    pub(crate) const fn of(pass: &GenerationPass) -> Self {
        Self {
            regime: pass.regime,
            information: pass.information,
            contribution: CorpusContributionV1 {
                templates: pass.contribution.templates as u64,
                references: pass.contribution.references as u64,
                gesture: pass.contribution.gesture,
            },
            candidates: pass.candidates,
            candidate_count: pass.candidate_count as u64,
        }
    }

    pub(crate) fn of_v1(pass: &GenerationPassV1) -> Self {
        Self {
            regime: pass.regime.into(),
            information: pass.information,
            contribution: pass.contribution,
            candidates: pass.candidates,
            candidate_count: pass.candidate_count,
        }
    }
}

/// A pass's record: what it claims happened. Output metadata lives here and
/// never in [`pass_information`], which is only what could cause the pass.
pub(crate) fn pass_record(claims: PassClaims) -> Fingerprint {
    let mut h = Hasher::new("griff.experiment.pass-record.v1");
    h.fingerprint(claims.information);
    h.bool(claims.regime.rhythms);
    h.bool(claims.regime.references);
    h.bool(claims.regime.gesture);
    h.u64(claims.contribution.templates);
    h.u64(claims.contribution.references);
    h.bool(claims.contribution.gesture);
    h.fingerprint(claims.candidates);
    h.u64(claims.candidate_count);
    h.finish()
}

/// A cell's outcome, as claimed.
#[derive(Debug, Clone, Copy)]
pub(crate) enum OutcomeClaims<'a> {
    Produced {
        content: Fingerprint,
        metrics: &'a [MetricValueV1],
        diagnostics: &'a [DiagnosticV1],
    },
    Refused(CellRefusalV1),
}

/// The record of a cell whose parts are already in wire form.
fn cell_record(
    placement: (u64, InformationRegime, u64),
    identities: (Fingerprint, Fingerprint),
    outcome: OutcomeClaims<'_>,
) -> Fingerprint {
    let ((variant, regime, pass), (requested, recipe)) = (placement, identities);
    let mut h = Hasher::new("griff.experiment.cell-record.v1");
    h.u64(variant);
    h.bool(regime.rhythms);
    h.bool(regime.references);
    h.bool(regime.gesture);
    h.u64(pass);
    h.fingerprint(requested);
    h.fingerprint(recipe);
    match outcome {
        OutcomeClaims::Produced {
            content,
            metrics,
            diagnostics,
        } => {
            h.u8(0);
            h.fingerprint(content);
            // The realization slot: absent is the only value version 1 has.
            h.u8(0);
            h.usize(metrics.len());
            for metric in metrics {
                metric.write(&mut h);
            }
            h.usize(diagnostics.len());
            for diagnostic in diagnostics {
                diagnostic.write(&mut h);
            }
        }
        OutcomeClaims::Refused(refusal) => {
            h.u8(1);
            refusal.write(&mut h);
        }
    }
    h.finish()
}

/// The record of an in-memory cell.
pub(crate) fn record_of_cell(cell: &Cell) -> Fingerprint {
    let placement = (wide(cell.variant), cell.regime, wide(cell.pass));
    let identities = (cell.requested, cell.recipe);
    match &cell.outcome {
        CellOutcome::Produced(result) => {
            let metrics: Vec<MetricValueV1> =
                result.metrics.iter().map(MetricValueV1::from).collect();
            let diagnostics: Vec<DiagnosticV1> =
                result.diagnostics.iter().map(|&d| d.into()).collect();
            cell_record(
                placement,
                identities,
                OutcomeClaims::Produced {
                    content: result.content,
                    metrics: &metrics,
                    diagnostics: &diagnostics,
                },
            )
        }
        CellOutcome::Refused(refusal) => cell_record(
            placement,
            identities,
            OutcomeClaims::Refused((*refusal).into()),
        ),
    }
}

/// The record of a recorded cell.
pub(crate) fn record_of_cell_v1(cell: &CellV1) -> Fingerprint {
    let placement = (cell.variant, cell.regime.into(), cell.pass);
    let identities = (cell.requested, cell.recipe);
    match &cell.outcome {
        CellOutcomeV1::Produced(result) => cell_record(
            placement,
            identities,
            OutcomeClaims::Produced {
                content: result.content,
                metrics: &result.metrics,
                diagnostics: &result.diagnostics,
            },
        ),
        CellOutcomeV1::Refused(refusal) => {
            cell_record(placement, identities, OutcomeClaims::Refused(*refusal))
        }
    }
}

/// Recomputes every record of `run` — each pass's, each cell's, and the run's
/// — from the run's own data. The one sealing path: the runner seals every run
/// it returns with exactly this.
pub(crate) fn seal(run: &mut ExperimentRun, labels: &[&str]) {
    for pass in &mut run.passes {
        pass.record = pass_record(PassClaims::of(pass));
    }
    for cell in &mut run.cells {
        cell.record = record_of_cell(cell);
    }
    run.record = run_record(
        RunClaims {
            spec: run.spec,
            source: run.source,
            evaluation: run.evaluation,
            population: run.corpus.as_ref().map(|p| p.whole),
            passes: &run.passes.iter().map(|p| p.record).collect::<Vec<_>>(),
            cells: &run.cells.iter().map(|c| c.record).collect::<Vec<_>>(),
        },
        labels,
    );
}

/// What a whole run records besides its spec identity.
#[derive(Debug, Clone, Copy)]
pub(crate) struct RunClaims<'a> {
    pub(crate) spec: Fingerprint,
    pub(crate) source: Fingerprint,
    pub(crate) evaluation: Option<Fingerprint>,
    pub(crate) population: Option<Fingerprint>,
    pub(crate) passes: &'a [Fingerprint],
    pub(crate) cells: &'a [Fingerprint],
}

/// The whole run's record. Variant labels are bound here — they stay out of
/// the spec identity, but not out of what the run claims.
pub(crate) fn run_record(claims: RunClaims<'_>, labels: &[&str]) -> Fingerprint {
    let mut h = Hasher::new("griff.experiment.run-record.v1");
    h.fingerprint(claims.spec);
    h.usize(labels.len());
    for label in labels {
        h.str(label);
    }
    h.fingerprint(claims.source);
    h.option_fingerprint(claims.evaluation);
    h.option_fingerprint(claims.population);
    h.usize(claims.passes.len());
    for &pass in claims.passes {
        h.fingerprint(pass);
    }
    h.usize(claims.cells.len());
    for &cell in claims.cells {
        h.fingerprint(cell);
    }
    h.finish()
}

/// What a generation pass could consume.
pub(crate) fn pass_information(
    source: Fingerprint,
    ask: Fingerprint,
    generator: PolicyRef<'_>,
    scorer: PolicyRef<'_>,
    offered: Channels,
) -> Fingerprint {
    let mut h = Hasher::new("griff.experiment.pass.v1");
    h.fingerprint(source);
    h.fingerprint(ask);
    write_policy(&mut h, generator);
    write_policy(&mut h, scorer);
    h.fingerprint(offered.rhythms);
    h.fingerprint(offered.references);
    h.fingerprint(offered.gesture);
    h.finish()
}

/// A cell's effective identity.
pub(crate) fn cell_recipe(
    information: Fingerprint,
    selector: PolicyRef<'_>,
    realizer: PolicyRef<'_>,
) -> Fingerprint {
    let mut h = Hasher::new("griff.experiment.cell.v1");
    h.fingerprint(information);
    write_policy(&mut h, selector);
    write_policy(&mut h, realizer);
    h.finish()
}

/// A cell's requested identity.
pub(crate) fn cell_request(
    inputs: (Fingerprint, Fingerprint),
    stages: Stages<'_>,
    regime: InformationRegime,
    population: Option<Fingerprint>,
) -> Fingerprint {
    let (source, ask) = inputs;
    let mut h = Hasher::new("griff.experiment.cell-request.v1");
    h.fingerprint(source);
    h.fingerprint(ask);
    write_policy(&mut h, stages.generator);
    write_policy(&mut h, stages.scorer);
    write_policy(&mut h, stages.selector);
    write_policy(&mut h, stages.realizer);
    h.bool(regime.rhythms);
    h.bool(regime.references);
    h.bool(regime.gesture);
    h.option_fingerprint(population);
    h.finish()
}

/// The evaluation context's identity.
pub(crate) fn evaluation_fingerprint(context: &EvaluationContextV1) -> Option<Fingerprint> {
    match context {
        EvaluationContextV1::None => None,
        EvaluationContextV1::GenerationAxes {
            evaluator,
            pitch_material,
            references,
        } => {
            let mut h = Hasher::new("griff.experiment.evaluation.v1");
            write_policy(&mut h, evaluator.into());
            pitch_material.write(&mut h);
            h.fingerprint(references_fingerprint_of(
                references.iter().map(ScoreV1::fingerprint),
            ));
            Some(h.finish())
        }
    }
}

/// The spec's identity. Variant labels are for people and are not hashed.
pub(crate) fn spec_fingerprint(spec: &ExperimentSpecV1) -> Fingerprint {
    let mut h = Hasher::new("griff.experiment.spec.v1");
    h.fingerprint(spec.ask.fingerprint());
    h.usize(spec.variants.len());
    for variant in &spec.variants {
        write_policy(&mut h, (&variant.generator.identity).into());
        write_policy(&mut h, (&variant.scorer.identity).into());
        write_policy(&mut h, (&variant.selector.identity).into());
        write_policy(&mut h, (&variant.realizer.identity).into());
    }
    h.usize(spec.regimes.len());
    for regime in &spec.regimes {
        h.bool(regime.rhythms);
        h.bool(regime.references);
        h.bool(regime.gesture);
    }
    h.option_fingerprint(evaluation_fingerprint(&spec.evaluation));
    h.finish()
}
