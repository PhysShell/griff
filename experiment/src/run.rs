//! Running an experiment: one generation pass per information need, one cell
//! per variant × regime, and the identities that make the run reproducible.

use griff_core::candidate_chain::{
    intact_s6_cost, plan_candidate_chain, ChainError, PlannedCandidateChain,
};
use griff_core::closure::closure_axes;
use griff_core::generate::GenerationStrategy;
use griff_core::generation_input::{
    ranked_candidates_from_view, CorpusContribution, CorpusMaterial, GenerationInputError,
    RankedSet,
};
use griff_core::novelty::{measure_novelty, novelty_axes};
use griff_core::score::Score;
use griff_core::scoring::Axes;

use crate::fingerprint::{
    ask_fingerprint, gesture_fingerprint, references_fingerprint, rhythms_fingerprint,
    score_fingerprint, Fingerprint, Hasher,
};
use crate::metric::{MetricIdentity, MetricKind, MetricValue, EVALUATOR_GENERATION_AXES};
use crate::regime::InformationRegime;
use crate::spec::{
    policy, EvaluationContext, ExperimentSpec, GeneratorPolicy, PolicyIdentity, RealizerPolicy,
    ScorerPolicy, SelectorPolicy, SpecError, VariantSpec,
};

/// The policy-objective name of a result's cost under `candidate_chain` v1 —
/// the chain's planned total, or the intact winner weighed as a chain.
pub const METRIC_CHAIN_COST: &str = "chain_cost";

/// The policy-objective name of the intact winner's `generation_rerank`
/// aggregate.
pub const METRIC_AGGREGATE: &str = "aggregate";

/// The inputs an experiment runs over — each an identity of its own, apart
/// from the spec.
#[derive(Debug, Clone, Copy)]
pub struct ExperimentInputs<'a> {
    /// The seed score every cell generates from.
    pub source: &'a Score,
    /// The bound, already prepared corpus population, if any. How it was
    /// selected (holdout included) is the caller's, and recorded by the caller.
    pub corpus: Option<&'a CorpusMaterial>,
}

/// The identity of the bound corpus population, channel by channel.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorpusSnapshot {
    /// The ordered rhythm-template palette.
    pub rhythms: Fingerprint,
    /// The ordered novelty reference set.
    pub references: Fingerprint,
    /// The gesture channel.
    pub gesture: Fingerprint,
    /// Rhythm templates in the population.
    pub rhythm_count: usize,
    /// References in the population.
    pub reference_count: usize,
    /// Whether the population carries a gesture.
    pub gesture_present: bool,
    /// Records the loader could not load, verbatim.
    pub skipped: Vec<String>,
    /// All three channels and the skipped list together.
    pub whole: Fingerprint,
}

/// The identity of `material` as a bound population.
#[must_use]
pub fn corpus_snapshot(material: &CorpusMaterial) -> CorpusSnapshot {
    let CorpusMaterial {
        rhythms,
        references,
        gesture,
        skipped,
    } = material;
    let (r, n, g) = (
        rhythms_fingerprint(rhythms),
        references_fingerprint(references),
        gesture_fingerprint(*gesture),
    );
    let mut h = Hasher::new("griff.experiment.corpus-snapshot.v1");
    h.fingerprint(r);
    h.fingerprint(n);
    h.fingerprint(g);
    h.usize(skipped.len());
    for name in skipped {
        h.str(name);
    }
    CorpusSnapshot {
        rhythms: r,
        references: n,
        gesture: g,
        rhythm_count: rhythms.len(),
        reference_count: references.len(),
        gesture_present: gesture.is_some(),
        skipped: skipped.clone(),
        whole: h.finish(),
    }
}

/// One generation pass: a ranked set produced once and shared by every cell
/// whose variant has the same generator and scorer in the same regime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GenerationPass {
    /// The regime this pass ran under.
    pub regime: InformationRegime,
    /// The generator stage.
    pub generator: PolicyIdentity,
    /// The scorer stage.
    pub scorer: PolicyIdentity,
    /// What the pass could consume: source, ask, stage identities, and the
    /// fingerprint of every channel **as offered** — a masked or absent channel
    /// hashes as empty, so a cell's identity depends only on the channels
    /// actually available to it.
    pub information: Fingerprint,
    /// What the corpus actually contributed.
    pub contribution: CorpusContribution,
    /// The ranked candidates, in rank order.
    pub candidates: Fingerprint,
    /// How many candidates were ranked.
    pub candidate_count: usize,
}

/// A typed realization of a result onto an instrument.
///
/// No policy produces one yet, so the type has no value: `None` is the only
/// realization a current result can hold, and nothing fabricates one. The
/// first realizing client (fingering, chord voicing) adds its variant.
#[allow(clippy::empty_enums)] // uninhabited on purpose: no realization exists yet
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RealizationArtifact {}

/// A typed fact about how a result was selected — a model fact, not a verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Diagnostic {
    /// The intact selector took this ranked candidate whole.
    Selected {
        /// Ordinal in the ranked set.
        candidate: usize,
        /// 1-based rank.
        rank: usize,
        /// Its strategy.
        strategy: GenerationStrategy,
        /// Its derived variant seed.
        variant_seed: u64,
    },
    /// The chain selector filled output bar `bar` (0-based) from this ranked
    /// candidate.
    ChainBar {
        /// The output bar, 0-based.
        bar: usize,
        /// The supplier's ordinal in the ranked set.
        candidate: usize,
        /// The supplier's 1-based rank.
        rank: usize,
        /// The supplier's strategy.
        strategy: GenerationStrategy,
        /// The supplier's derived variant seed.
        variant_seed: u64,
    },
}

/// One produced result.
#[derive(Debug, Clone, PartialEq)]
pub struct ExperimentResult {
    /// The result's score.
    pub score: Score,
    /// Its content fingerprint.
    pub content: Fingerprint,
    /// Its realization — always `None` until a realizing policy exists.
    pub realization: Option<RealizationArtifact>,
    /// Evaluation metrics and policy objectives, each with its identity.
    pub metrics: Vec<MetricValue>,
    /// How it was selected.
    pub diagnostics: Vec<Diagnostic>,
}

impl ExperimentResult {
    /// The metric named `name`, of `kind`, if measured.
    #[must_use]
    pub fn metric(&self, kind: MetricKind, name: &str) -> Option<&MetricValue> {
        self.metrics
            .iter()
            .find(|m| m.identity.kind == kind && m.identity.name == name)
    }
}

/// Why a cell has no result. Typed, never a fake one.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CellRefusal {
    /// The ranked set held no candidate to select.
    EmptySet,
    /// The chain planner refused the set.
    Chain(ChainError),
}

/// A cell's outcome.
#[derive(Debug, Clone, PartialEq)]
pub enum CellOutcome {
    /// A result.
    Produced(Box<ExperimentResult>),
    /// No result, and why.
    Refused(CellRefusal),
}

/// One variant under one regime.
#[derive(Debug, Clone, PartialEq)]
pub struct Cell {
    /// Index into the spec's variants.
    pub variant: usize,
    /// The requested regime (what was asked; the pass says what was taken).
    pub regime: InformationRegime,
    /// Index into [`ExperimentRun::passes`].
    pub pass: usize,
    /// What produced this cell: the pass's information plus the selector and
    /// realizer identities.
    pub recipe: Fingerprint,
    /// The result, or the typed reason there is none.
    pub outcome: CellOutcome,
}

/// A whole run — immutable once returned.
#[derive(Debug, Clone, PartialEq)]
pub struct ExperimentRun {
    /// The spec's fingerprint.
    pub spec: Fingerprint,
    /// The source's fingerprint.
    pub source: Fingerprint,
    /// The bound population's identity; `None` without a corpus.
    pub corpus: Option<CorpusSnapshot>,
    /// The evaluation context's fingerprint; `None` without an evaluator.
    pub evaluation: Option<Fingerprint>,
    /// Generation passes, in first-needed order.
    pub passes: Vec<GenerationPass>,
    /// Cells in variant × regime order: variant 0 under every regime, then
    /// variant 1, …
    pub cells: Vec<Cell>,
}

impl ExperimentRun {
    /// The cell of variant `variant` under `regime`.
    #[must_use]
    pub fn cell(&self, variant: usize, regime: InformationRegime) -> Option<&Cell> {
        self.cells
            .iter()
            .find(|cell| cell.variant == variant && cell.regime == regime)
    }
}

/// Why a run could not start or finish.
#[derive(Debug)]
pub enum RunError {
    /// The spec is invalid.
    Spec(SpecError),
    /// The source could not seed a generation pass.
    Generation(GenerationInputError),
}

/// Runs every cell of `spec` over `inputs`.
///
/// # Errors
/// [`RunError::Spec`] for an invalid spec, [`RunError::Generation`] when the
/// source cannot seed a pass. A refused selection is a cell outcome, not an
/// error.
pub fn run_experiment(
    spec: &ExperimentSpec,
    inputs: &ExperimentInputs<'_>,
) -> Result<ExperimentRun, RunError> {
    spec.validate().map_err(RunError::Spec)?;
    let context = RunContext {
        spec,
        inputs,
        source: score_fingerprint(inputs.source),
        ask: ask_fingerprint(&spec.ask),
        evaluation: spec.evaluation.fingerprint(),
    };

    let mut live: Vec<LivePass> = Vec::new();
    let mut cells = Vec::with_capacity(spec.variants.len().saturating_mul(spec.regimes.len()));
    for (variant_index, variant) in spec.variants.iter().enumerate() {
        for &regime in &spec.regimes {
            // One pass per regime and generation stages: a variant that differs
            // only after the ranked set reads the pass another variant made.
            if !live.iter().any(|p| p.serves(regime, variant)) {
                live.push(LivePass::run(&context, regime, variant)?);
            }
            let Some(pass) = live.iter().position(|p| p.serves(regime, variant)) else {
                continue;
            };
            let Some(live_pass) = live.get_mut(pass) else {
                continue;
            };
            let mut h = Hasher::new("griff.experiment.cell.v1");
            h.fingerprint(live_pass.record.information);
            policy(&mut h, variant.selector.identity());
            policy(&mut h, variant.realizer.identity());
            cells.push(Cell {
                variant: variant_index,
                regime,
                pass,
                recipe: h.finish(),
                outcome: live_pass.select(variant, &context),
            });
        }
    }

    Ok(ExperimentRun {
        spec: spec.fingerprint(),
        source: context.source,
        corpus: inputs.corpus.map(corpus_snapshot),
        evaluation: context.evaluation,
        passes: live.into_iter().map(|p| p.record).collect(),
        cells,
    })
}

/// What every pass and cell of one run shares.
struct RunContext<'a> {
    spec: &'a ExperimentSpec,
    inputs: &'a ExperimentInputs<'a>,
    source: Fingerprint,
    ask: Fingerprint,
    evaluation: Option<Fingerprint>,
}

/// A pass while the run is still selecting from it: its record, and the ranked
/// set itself, which dies with the run.
struct LivePass {
    generator: GeneratorPolicy,
    scorer: ScorerPolicy,
    view_regime: InformationRegime,
    set: RankedSet,
    record: GenerationPass,
    /// The chain planned from `set`, once some cell needed it.
    chain: Option<Result<PlannedCandidateChain, ChainError>>,
    /// The intact winner weighed under the chain policy, once needed.
    intact_cost: Option<Result<f64, ChainError>>,
}

impl LivePass {
    /// Whether this pass is the one `variant` needs under `regime`.
    fn serves(&self, regime: InformationRegime, variant: &VariantSpec) -> bool {
        self.view_regime == regime
            && self.generator == variant.generator
            && self.scorer == variant.scorer
    }

    /// Runs the one generation pass `variant` needs under `regime`.
    fn run(
        context: &RunContext<'_>,
        regime: InformationRegime,
        variant: &VariantSpec,
    ) -> Result<Self, RunError> {
        let view = regime.view(context.inputs.corpus);
        // Every arm of the generator and scorer axes enters the one shared
        // generation path; a new arm adds its own adapter here.
        let set = match (variant.generator, variant.scorer) {
            (GeneratorPolicy::S6CandidateSet, ScorerPolicy::GenerationRerankV1) => {
                ranked_candidates_from_view(context.inputs.source, view, &context.spec.ask, None)
                    .map_err(RunError::Generation)?
            }
        };

        let mut h = Hasher::new("griff.experiment.pass.v1");
        h.fingerprint(context.source);
        h.fingerprint(context.ask);
        policy(&mut h, variant.generator.identity());
        policy(&mut h, variant.scorer.identity());
        // Channels as offered: a masked or absent channel hashes as empty.
        h.fingerprint(rhythms_fingerprint(view.rhythms));
        h.fingerprint(references_fingerprint(view.references));
        h.fingerprint(gesture_fingerprint(view.gesture));
        let information = h.finish();

        let record = GenerationPass {
            regime,
            generator: variant.generator.identity(),
            scorer: variant.scorer.identity(),
            information,
            contribution: CorpusContribution::of_pass(view, &set),
            candidates: candidates_fingerprint(&set),
            candidate_count: set.ranked.len(),
        };
        Ok(Self {
            generator: variant.generator,
            scorer: variant.scorer,
            view_regime: regime,
            set,
            record,
            chain: None,
            intact_cost: None,
        })
    }

    /// Selects `variant`'s result from this pass's ranked set.
    fn select(&mut self, variant: &VariantSpec, context: &RunContext<'_>) -> CellOutcome {
        let information = self.record.information;
        let objective = |name: &'static str, owner: PolicyIdentity, value: f64| MetricValue {
            identity: MetricIdentity {
                kind: MetricKind::PolicyObjective,
                name,
                owner,
                context: information,
            },
            value,
        };
        let chain_policy = SelectorPolicy::GlobalChainV1.identity();

        let (score, mut metrics, diagnostics) = match variant.selector {
            SelectorPolicy::IntactTop => {
                let Some(winner) = self.set.ranked.first() else {
                    return CellOutcome::Refused(CellRefusal::EmptySet);
                };
                let mut metrics = vec![objective(
                    METRIC_AGGREGATE,
                    variant.scorer.identity(),
                    winner.aggregate(),
                )];
                let set = &self.set;
                if let Ok(cost) = *self.intact_cost.get_or_insert_with(|| intact_s6_cost(set)) {
                    metrics.push(objective(METRIC_CHAIN_COST, chain_policy, cost));
                }
                let selected = Diagnostic::Selected {
                    candidate: 0,
                    rank: 1,
                    strategy: winner.value.strategy,
                    variant_seed: winner.value.seed.0,
                };
                (winner.value.score.clone(), metrics, vec![selected])
            }
            SelectorPolicy::GlobalChainV1 => {
                let set = &self.set;
                let plan = match self.chain.get_or_insert_with(|| plan_candidate_chain(set)) {
                    Ok(plan) => plan,
                    Err(error) => return CellOutcome::Refused(CellRefusal::Chain(*error)),
                };
                (
                    plan.score.clone(),
                    vec![objective(METRIC_CHAIN_COST, chain_policy, plan.total_cost)],
                    chain_diagnostics(plan),
                )
            }
        };
        metrics.extend(evaluate(&score, context));

        let realization = match variant.realizer {
            RealizerPolicy::None => None,
        };
        CellOutcome::Produced(Box::new(ExperimentResult {
            content: score_fingerprint(&score),
            score,
            realization,
            metrics,
            diagnostics,
        }))
    }
}

/// One supplier per output bar, from the chain's own trace.
fn chain_diagnostics(plan: &PlannedCandidateChain) -> Vec<Diagnostic> {
    plan.steps
        .iter()
        .map(|step| Diagnostic::ChainBar {
            bar: step.state.bar,
            candidate: step.state.candidate,
            rank: step.state.rank,
            strategy: step.state.strategy,
            variant_seed: step.state.variant_seed.0,
        })
        .collect()
}

/// The run's evaluator over `score` in its one fixed context — nothing without
/// an evaluator, and nothing for an axis the evaluator cannot measure.
fn evaluate(score: &Score, context: &RunContext<'_>) -> Vec<MetricValue> {
    let (
        EvaluationContext::GenerationAxes {
            pitch_material,
            references,
        },
        Some(fingerprint),
    ) = (&context.spec.evaluation, context.evaluation)
    else {
        return Vec::new();
    };
    let closure = closure_axes(score, 0, pitch_material).ok();
    let novelty = measure_novelty(score, 0, references)
        .ok()
        .map(|report| novelty_axes(&report));
    [closure, novelty]
        .iter()
        .flatten()
        .flat_map(|axes| evaluation_metrics(axes, fingerprint))
        .collect()
}

/// Every axis of `axes` as an evaluation measured by the generation-axes
/// evaluator in `context`.
fn evaluation_metrics(axes: &Axes, context: Fingerprint) -> impl Iterator<Item = MetricValue> + '_ {
    axes.iter().map(move |axis| MetricValue {
        identity: MetricIdentity {
            kind: MetricKind::Evaluation,
            name: axis.label,
            owner: EVALUATOR_GENERATION_AXES,
            context,
        },
        value: axis.value,
    })
}

/// The ranked candidates in rank order: strategy, seed, aggregate, every axis,
/// the carved gesture, and the score.
fn candidates_fingerprint(set: &RankedSet) -> Fingerprint {
    let mut h = Hasher::new("griff.experiment.candidates.v1");
    h.usize(set.ranked.len());
    for candidate in &set.ranked {
        h.str(strategy_name(candidate.value.strategy));
        h.u64(candidate.value.seed.0);
        h.f64(candidate.aggregate());
        h.usize(candidate.axes.iter().len());
        for axis in &candidate.axes {
            h.str(axis.label);
            h.f64(axis.value);
        }
        h.fingerprint(gesture_fingerprint(candidate.value.gesture));
        h.fingerprint(score_fingerprint(&candidate.value.score));
    }
    h.finish()
}

/// A strategy's stable name for hashing — exhaustive, so a new strategy is a
/// compile error here rather than an unhashed one.
const fn strategy_name(strategy: GenerationStrategy) -> &'static str {
    match strategy {
        GenerationStrategy::RhythmCopyPitchSubstitute => "rhythm_copy_pitch_substitute",
        GenerationStrategy::MotifTransposeVariation => "motif_transpose_variation",
        GenerationStrategy::ConstrainedRandomWalk => "constrained_random_walk",
        GenerationStrategy::ShuffleMotifs => "shuffle_motifs",
        GenerationStrategy::RepeatVariation => "repeat_variation",
    }
}
