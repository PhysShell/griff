//! The persistent experiment bundle, version 1 (ADR-0034 decision 9).
//!
//! A bundle is a whole run written down: the full spec (labels, policies and
//! their recorded identities, regimes, the evaluation context with its
//! references), the source score, the bound population's identity, every pass,
//! and every cell with both its requested and its effective identity, its
//! score, metrics and diagnostics. It is self-explanatory: every fingerprint it
//! records can be recomputed from what it records, and loading does exactly
//! that.
//!
//! **Loading never generates.** [`ExperimentBundleV1::from_json`] parses,
//! validates the projections, and verifies identities; nothing in this module
//! reaches a generator, a scorer, or a planner. A recorded score is shown as
//! recorded.
//!
//! Wire rules:
//! - Scores and other model values travel as the canonical projection V1
//!   ([`crate::projection`]), the same one every fingerprint walks.
//! - Names are owned strings on the wire. The runtime's `&'static str`
//!   identities are rebuilt only from the closed vocabulary this version knows;
//!   an unknown name is a typed refusal, never a leaked allocation.
//! - A realization can only be absent: [`RealizationV1`] has no value.
//! - Unknown fields are refused, never dropped.
//! - Nothing of the run is left out: [`ExperimentBundleV1::run`] rebuilds the
//!   in-memory run exactly.

use griff_core::score::Score;
use serde::{Deserialize, Serialize};

use crate::fingerprint::Fingerprint;
use crate::projection::{GenerationAskV1, PitchMaterialV1, ProjectionError, ScoreV1};
use crate::run::ExperimentRun;
use crate::spec::{ExperimentSpec, PolicyIdentity, SpecError};

/// The schema marker every bundle carries.
pub const BUNDLE_SCHEMA: &str = "griff.experiment-bundle";

/// The bundle shape version this code reads and writes.
pub const BUNDLE_VERSION: u32 = 1;

// ── spec ──────────────────────────────────────────────────────────────────────

/// A policy identity, as recorded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PolicyIdentityV1 {
    /// Stable identifier.
    pub id: String,
    /// Version at the time of the run.
    pub version: u32,
}

/// A pipeline stage: which policy arm, and the identity it had when the run
/// was made. The identity is recorded, not re-derived: a later version bump
/// must not rewrite what an old run was.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StageV1<P> {
    /// The policy arm.
    pub policy: P,
    /// Its identity at run time.
    pub identity: PolicyIdentityV1,
}

/// A generator arm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GeneratorPolicyV1 {
    /// Every S6 strategy × seed variants.
    S6CandidateSet,
}

/// A scorer arm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScorerPolicyV1 {
    /// `generation_rerank` v1.
    GenerationRerankV1,
}

/// A selector arm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SelectorPolicyV1 {
    /// S6 Intact.
    IntactTop,
    /// S7 Global Chain.
    GlobalChainV1,
}

/// A realizer arm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RealizerPolicyV1 {
    /// No realization.
    None,
}

/// A variant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VariantSpecV1 {
    /// Its human label.
    pub label: String,
    /// Generator stage.
    pub generator: StageV1<GeneratorPolicyV1>,
    /// Scorer stage.
    pub scorer: StageV1<ScorerPolicyV1>,
    /// Selector stage.
    pub selector: StageV1<SelectorPolicyV1>,
    /// Realizer stage.
    pub realizer: StageV1<RealizerPolicyV1>,
}

/// An information regime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(clippy::struct_excessive_bools)] // one bool per channel is the regime
pub struct InformationRegimeV1 {
    /// Rhythm templates.
    pub rhythms: bool,
    /// Novelty references.
    pub references: bool,
    /// Gesture.
    pub gesture: bool,
}

/// The evaluation context.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvaluationContextV1 {
    /// No evaluator.
    None,
    /// The generation-axes evaluator in a supplied context.
    GenerationAxes {
        /// The evaluator's identity at run time.
        evaluator: PolicyIdentityV1,
        /// The scale closure was measured against.
        pitch_material: PitchMaterialV1,
        /// The references novelty was measured against.
        references: Vec<ScoreV1>,
    },
}

/// A whole spec.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExperimentSpecV1 {
    /// The ask.
    pub ask: GenerationAskV1,
    /// The variant axis.
    pub variants: Vec<VariantSpecV1>,
    /// The information axis.
    pub regimes: Vec<InformationRegimeV1>,
    /// The evaluation context.
    pub evaluation: EvaluationContextV1,
}

// ── run ───────────────────────────────────────────────────────────────────────

/// The run-level identities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunIdentitiesV1 {
    /// The spec's fingerprint.
    pub spec: Fingerprint,
    /// The source's fingerprint.
    pub source: Fingerprint,
    /// The evaluation context's fingerprint, when there is one.
    pub evaluation: Option<Fingerprint>,
}

/// The bound population's identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CorpusSnapshotV1 {
    /// Rhythm-template palette fingerprint.
    pub rhythms: Fingerprint,
    /// Reference-set fingerprint.
    pub references: Fingerprint,
    /// Gesture fingerprint.
    pub gesture: Fingerprint,
    /// Templates in the population.
    pub rhythm_count: u64,
    /// References in the population.
    pub reference_count: u64,
    /// Whether it carries a gesture.
    pub gesture_present: bool,
    /// Records the loader skipped.
    pub skipped: Vec<String>,
    /// The whole snapshot.
    pub whole: Fingerprint,
}

/// What a corpus actually contributed to a pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CorpusContributionV1 {
    /// Corpus templates rotated.
    pub templates: u64,
    /// References measured against.
    pub references: u64,
    /// Whether a corpus gesture was carved.
    pub gesture: bool,
}

/// A generation pass.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GenerationPassV1 {
    /// Its requested regime.
    pub regime: InformationRegimeV1,
    /// Generator identity.
    pub generator: PolicyIdentityV1,
    /// Scorer identity.
    pub scorer: PolicyIdentityV1,
    /// What it could consume.
    pub information: Fingerprint,
    /// What the corpus contributed.
    pub contribution: CorpusContributionV1,
    /// The ranked candidates.
    pub candidates: Fingerprint,
    /// How many were ranked.
    pub candidate_count: u64,
}

/// A realization — no value exists in version 1, so the only representable
/// realization is its absence (`null`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[allow(clippy::empty_enums)] // uninhabited on purpose, like the runtime type
pub enum RealizationV1 {}

/// A metric kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MetricKindV1 {
    /// An evaluation.
    Evaluation,
    /// A policy objective.
    PolicyObjective,
}

/// A measured value with its comparability identity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetricValueV1 {
    /// Evaluation or objective.
    pub kind: MetricKindV1,
    /// Axis or objective name, owned.
    pub name: String,
    /// Evaluator or policy.
    pub owner: PolicyIdentityV1,
    /// The context it was measured in.
    pub context: Fingerprint,
    /// The value (always finite).
    pub value: f64,
}

/// An S6 strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StrategyV1 {
    /// Rhythm copy, pitch substitute.
    RhythmCopyPitchSubstitute,
    /// Motif transpose variation.
    MotifTransposeVariation,
    /// Constrained random walk.
    ConstrainedRandomWalk,
    /// Shuffle motifs.
    ShuffleMotifs,
    /// Repeat variation.
    RepeatVariation,
}

/// A selection diagnostic.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiagnosticV1 {
    /// The intact selector took a ranked candidate whole.
    Selected {
        /// Ordinal.
        candidate: u64,
        /// 1-based rank.
        rank: u64,
        /// Strategy.
        strategy: StrategyV1,
        /// Variant seed.
        variant_seed: u64,
    },
    /// The chain selector filled a bar.
    ChainBar {
        /// Output bar, 0-based.
        bar: u64,
        /// Supplier ordinal.
        candidate: u64,
        /// Supplier rank.
        rank: u64,
        /// Supplier strategy.
        strategy: StrategyV1,
        /// Supplier variant seed.
        variant_seed: u64,
    },
}

/// A produced result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExperimentResultV1 {
    /// Its score.
    pub score: ScoreV1,
    /// Its content fingerprint.
    pub content: Fingerprint,
    /// Always absent in version 1.
    pub realization: Option<RealizationV1>,
    /// Its metrics.
    pub metrics: Vec<MetricValueV1>,
    /// Its diagnostics.
    pub diagnostics: Vec<DiagnosticV1>,
}

/// A master-bar field a chain refusal names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MasterBarFieldV1 {
    /// Index.
    Index,
    /// Tick range.
    TickRange,
    /// Time signature.
    TimeSignature,
    /// Tempo.
    Tempo,
    /// Repeat.
    Repeat,
}

/// A track field a chain refusal names.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrackFieldV1 {
    /// Name.
    Name,
    /// Channel.
    Channel,
    /// Tuning.
    Tuning,
    /// Voice count.
    VoiceCount,
    /// Voice id.
    VoiceId,
}

/// A layered-path state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StateIdV1 {
    /// Layer.
    pub layer: u64,
    /// Ordinal within the layer.
    pub ordinal: u64,
}

/// A layered-path edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EdgeIdV1 {
    /// From state.
    pub from: StateIdV1,
    /// To state.
    pub to: StateIdV1,
}

/// A boundary-fact refusal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TransitionFactErrorV1 {
    /// The leaving bar is missing.
    MissingFromBar {
        /// Bar.
        bar: u64,
        /// Bars available.
        bars: u64,
    },
    /// The entering bar is missing.
    MissingToBar {
        /// Bar.
        bar: u64,
        /// Bars available.
        bars: u64,
    },
}

/// A layered-path refusal. Non-finite costs travel as their IEEE-754 bits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PathErrorV1 {
    /// No layers.
    NoLayers,
    /// An empty layer.
    EmptyLayer {
        /// Layer.
        layer: u64,
    },
    /// Wrong transition-table count.
    TransitionCount {
        /// Expected.
        expected: u64,
        /// Found.
        found: u64,
    },
    /// Wrong transition-table shape.
    TransitionShape {
        /// Layer.
        layer: u64,
        /// Expected shape.
        expected: (u64, u64),
        /// Found shape.
        found: (u64, u64),
    },
    /// A non-finite local cost.
    NonFiniteLocal {
        /// State.
        state: StateIdV1,
        /// Cost bits.
        cost_bits: u64,
    },
    /// A non-finite transition cost.
    NonFiniteTransition {
        /// Edge.
        edge: EdgeIdV1,
        /// Cost bits.
        cost_bits: u64,
    },
    /// A non-finite accumulated cost.
    NonFiniteAccumulation {
        /// State.
        state: StateIdV1,
        /// Cost bits.
        cost_bits: u64,
    },
    /// k = 0.
    KZero,
    /// Minimum distance 0.
    MinDistanceZero,
    /// Unsatisfiable minimum distance.
    MinDistanceUnsatisfiable {
        /// Minimum distance.
        min_distance: u64,
        /// Layers.
        layers: u64,
    },
}

/// A chain refusal, mirrored variant for variant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChainErrorV1 {
    /// Empty set.
    EmptySet,
    /// No bars.
    NoBars,
    /// Bar count mismatch.
    BarCountMismatch {
        /// Candidate.
        candidate: u64,
        /// Expected.
        expected: u64,
        /// Found.
        found: u64,
    },
    /// Tick resolution mismatch.
    PpqMismatch {
        /// Candidate.
        candidate: u64,
        /// Expected.
        expected: u16,
        /// Found.
        found: u16,
    },
    /// Master bar mismatch.
    MasterBarMismatch {
        /// Candidate.
        candidate: u64,
        /// Bar.
        bar: u64,
        /// Field.
        field: MasterBarFieldV1,
    },
    /// Track count mismatch.
    TrackCountMismatch {
        /// Candidate.
        candidate: u64,
        /// Expected.
        expected: u64,
        /// Found.
        found: u64,
    },
    /// Track metadata mismatch.
    TrackMetadataMismatch {
        /// Candidate.
        candidate: u64,
        /// Track.
        track: u64,
        /// Field.
        field: TrackFieldV1,
    },
    /// Source metadata mismatch.
    SourceMetaMismatch {
        /// Candidate.
        candidate: u64,
    },
    /// Loss report mismatch.
    LossReportMismatch {
        /// Candidate.
        candidate: u64,
    },
    /// Material crosses a bar line.
    CrossBarMaterial {
        /// Candidate.
        candidate: u64,
        /// Bar.
        bar: u64,
    },
    /// An empty event group.
    EmptyEventGroup {
        /// Candidate.
        candidate: u64,
    },
    /// Material outside the timeline.
    MaterialOutsideTimeline {
        /// Candidate.
        candidate: u64,
        /// Tick.
        tick: u32,
    },
    /// Missing material.
    MissingMaterial {
        /// Candidate.
        candidate: u64,
        /// Track.
        track: u64,
        /// Voice.
        voice: u64,
        /// Bar.
        bar: u64,
    },
    /// A boundary fact could not be measured.
    BoundaryFact(TransitionFactErrorV1),
    /// The layered path refused.
    Path(PathErrorV1),
}

/// Why a cell has no result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CellRefusalV1 {
    /// Nothing to select.
    EmptySet,
    /// The chain planner refused.
    Chain(ChainErrorV1),
}

/// A cell's outcome.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CellOutcomeV1 {
    /// A result.
    Produced(Box<ExperimentResultV1>),
    /// A typed refusal.
    Refused(CellRefusalV1),
}

/// A cell.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CellV1 {
    /// Variant index.
    pub variant: u64,
    /// Requested regime.
    pub regime: InformationRegimeV1,
    /// Pass index.
    pub pass: u64,
    /// What was asked.
    pub requested: Fingerprint,
    /// What produced it.
    pub recipe: Fingerprint,
    /// Its outcome.
    pub outcome: CellOutcomeV1,
}

/// A whole run, written down.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExperimentBundleV1 {
    /// [`BUNDLE_SCHEMA`].
    pub schema: String,
    /// [`BUNDLE_VERSION`].
    pub version: u32,
    /// The whole spec.
    pub spec: ExperimentSpecV1,
    /// The source score.
    pub source: ScoreV1,
    /// Run-level identities.
    pub identities: RunIdentitiesV1,
    /// The bound population's identity, when one was bound.
    pub population: Option<CorpusSnapshotV1>,
    /// Passes.
    pub passes: Vec<GenerationPassV1>,
    /// Cells, variant × regime.
    pub cells: Vec<CellV1>,
}

// ── errors ────────────────────────────────────────────────────────────────────

/// Which recorded identity disagrees with what the bundle records.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mismatch {
    /// The spec fingerprint.
    Spec,
    /// The source fingerprint.
    Source,
    /// The evaluation-context fingerprint.
    Evaluation,
    /// The population's whole fingerprint.
    Population,
    /// A pass's information.
    PassInformation {
        /// The pass.
        pass: usize,
    },
    /// A cell names a variant the spec does not have.
    CellVariant {
        /// The cell.
        cell: usize,
    },
    /// A cell names a pass the run does not have, or one of another regime or
    /// other generation stages.
    CellPass {
        /// The cell.
        cell: usize,
    },
    /// A cell's requested identity.
    CellRequested {
        /// The cell.
        cell: usize,
    },
    /// A cell's recipe.
    CellRecipe {
        /// The cell.
        cell: usize,
    },
    /// A result's content fingerprint.
    CellContent {
        /// The cell.
        cell: usize,
    },
    /// A metric's context.
    MetricContext {
        /// The cell.
        cell: usize,
        /// The metric's position.
        metric: usize,
    },
}

/// Which spec stage a recorded identity drifted on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// Generator.
    Generator,
    /// Scorer.
    Scorer,
    /// Selector.
    Selector,
    /// Realizer.
    Realizer,
    /// Evaluator.
    Evaluator,
}

/// Why a bundle cannot be written, read, or rebuilt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BundleError {
    /// The JSON does not parse into bundle version 1 (including unknown
    /// fields and a present realization); the parser's message.
    Malformed(String),
    /// Another schema.
    UnknownSchema(String),
    /// Another version.
    UnsupportedVersion(u32),
    /// A recorded model value the model cannot hold.
    Projection(ProjectionError),
    /// A recorded identity that is not what the bundle's own data hashes to.
    IdentityMismatch(Mismatch),
    /// A name outside this version's closed vocabulary.
    UnknownName(String),
    /// A recorded stage identity that the current code's policy arm no longer
    /// has — the spec cannot be rebuilt as the same experiment.
    IdentityDrift {
        /// The stage.
        stage: Stage,
        /// As recorded.
        recorded: PolicyIdentityV1,
    },
    /// The rebuilt spec is invalid.
    Spec(SpecError),
    /// The spec or source handed to [`ExperimentBundleV1::from_run`] is not the
    /// one the run was made from.
    NotThisRun,
    /// A metric value that JSON cannot carry exactly.
    NonFiniteMetric {
        /// The cell.
        cell: usize,
        /// The metric's position.
        metric: usize,
    },
}

// ── operations ────────────────────────────────────────────────────────────────

impl ExperimentBundleV1 {
    /// Writes down `run`, made from `spec` over `source`.
    ///
    /// # Errors
    /// [`BundleError::NotThisRun`] when `spec` or `source` does not match the
    /// run's identities; [`BundleError::NonFiniteMetric`] for a value JSON
    /// cannot carry exactly.
    pub fn from_run(
        spec: &ExperimentSpec,
        source: &Score,
        run: &ExperimentRun,
    ) -> Result<Self, BundleError> {
        let _ = (spec, source, run);
        Err(BundleError::NotThisRun)
    }

    /// The bundle as pretty, deterministic JSON.
    #[must_use]
    pub fn to_json(&self) -> String {
        let _ = self;
        String::new()
    }

    /// Reads a bundle: parse, check schema and version, validate every
    /// projection, verify every identity. Never generates.
    ///
    /// # Errors
    /// The first [`BundleError`] found.
    pub fn from_json(json: &str) -> Result<Self, BundleError> {
        let _ = json;
        Err(BundleError::Malformed(String::new()))
    }

    /// Recomputes every identity from the bundle's own data.
    ///
    /// # Errors
    /// [`BundleError::IdentityMismatch`] naming the first disagreement.
    pub const fn verify(&self) -> Result<(), BundleError> {
        let _ = self;
        Ok(())
    }

    /// The recorded spec as a runnable spec, when the current code still has
    /// every recorded stage identity.
    ///
    /// # Errors
    /// [`BundleError::IdentityDrift`], [`BundleError::Projection`],
    /// [`BundleError::Spec`].
    pub fn spec(&self) -> Result<ExperimentSpec, BundleError> {
        let _ = self;
        Err(BundleError::NotThisRun)
    }

    /// The recorded source score.
    ///
    /// # Errors
    /// [`BundleError::Projection`].
    pub fn source_score(&self) -> Result<Score, BundleError> {
        let _ = self;
        Err(BundleError::NotThisRun)
    }

    /// The in-memory run this bundle records, rebuilt exactly.
    ///
    /// # Errors
    /// [`BundleError::Projection`], [`BundleError::UnknownName`].
    pub fn run(&self) -> Result<ExperimentRun, BundleError> {
        let _ = self;
        Err(BundleError::NotThisRun)
    }
}

impl From<PolicyIdentity> for PolicyIdentityV1 {
    fn from(identity: PolicyIdentity) -> Self {
        Self {
            id: identity.id.to_owned(),
            version: identity.version,
        }
    }
}
