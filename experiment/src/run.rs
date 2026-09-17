//! Running an experiment: one generation pass per information need, one cell
//! per variant × regime, and the identities that make the run reproducible.

use griff_core::candidate_chain::ChainError;
use griff_core::generate::GenerationStrategy;
use griff_core::generation_input::{CorpusContribution, CorpusMaterial, GenerationInputError};
use griff_core::score::Score;

use crate::fingerprint::Fingerprint;
use crate::metric::MetricValue;
use crate::regime::InformationRegime;
use crate::spec::{ExperimentSpec, PolicyIdentity, SpecError};

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
    let _ = material;
    CorpusSnapshot {
        rhythms: Fingerprint([0; 32]),
        references: Fingerprint([0; 32]),
        gesture: Fingerprint([0; 32]),
        rhythm_count: 0,
        reference_count: 0,
        gesture_present: false,
        skipped: Vec::new(),
        whole: Fingerprint([0; 32]),
    }
}

/// One generation pass: a ranked set produced once and shared by every cell
/// whose variant has the same generator and scorer in the same regime.
#[derive(Debug, Clone, PartialEq, Eq)]
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
#[allow(clippy::empty_enum)] // uninhabited on purpose: no realization exists yet
#[derive(Debug, Clone, PartialEq, Eq)]
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
    pub fn metric(&self, kind: crate::MetricKind, name: &str) -> Option<&MetricValue> {
        let _ = (kind, name);
        None
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
        let _ = (variant, regime);
        None
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
    let _ = (spec, inputs);
    Err(RunError::Spec(SpecError::NoVariants))
}
