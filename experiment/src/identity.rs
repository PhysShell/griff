//! The run identities, defined once.
//!
//! The runner computes these while it runs; a loaded bundle recomputes them
//! from what it records. Both call the functions here, over the canonical
//! projection, so a run's identity and its bundle's verification cannot
//! disagree about what an identity is (ADR-0034 decisions 4 and 5).

use crate::bundle::{EvaluationContextV1, ExperimentSpecV1, PolicyIdentityV1};
use crate::fingerprint::{
    gesture_fingerprint, references_fingerprint, references_fingerprint_of, rhythms_fingerprint,
    Fingerprint, Hasher,
};
use crate::projection::ScoreV1;
use crate::regime::InformationRegime;
use crate::run::CorpusSnapshot;
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

/// The whole identity of a bound population.
pub(crate) fn snapshot_whole(channels: Channels, skipped: &[String]) -> Fingerprint {
    let mut h = Hasher::new("griff.experiment.corpus-snapshot.v1");
    h.fingerprint(channels.rhythms);
    h.fingerprint(channels.references);
    h.fingerprint(channels.gesture);
    h.usize(skipped.len());
    for name in skipped {
        h.str(name);
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
