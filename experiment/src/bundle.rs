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

use griff_core::candidate_chain::{ChainError, MasterBarField, TrackField, TransitionFactError};
use griff_core::generate::GenerationStrategy;
use griff_core::generation_input::CorpusContribution;
use griff_core::layered_path::{EdgeId, PathError, StateId};
use griff_core::rerank::RERANK_AXIS_LABELS;
use griff_core::score::Score;
use serde::{Deserialize, Serialize};

use crate::fingerprint::{score_fingerprint, Fingerprint};
use crate::identity::{self, Channels, PolicyRef, Stages};
use crate::metric::{MetricIdentity, MetricKind, MetricValue, EVALUATOR_GENERATION_AXES};
use crate::projection::{count, GenerationAskV1, PitchMaterialV1, ProjectionError, ScoreV1};
use crate::regime::InformationRegime;
use crate::run::{
    Cell, CellOutcome, CellRefusal, CorpusSnapshot, Diagnostic, ExperimentResult, ExperimentRun,
    GenerationPass, METRIC_AGGREGATE, METRIC_CHAIN_COST,
};
use crate::spec::{
    EvaluationContext, ExperimentSpec, GeneratorPolicy, PolicyIdentity, RealizerPolicy,
    ScorerPolicy, SelectorPolicy, SpecError, VariantSpec,
};

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

/// Just enough of a bundle to know whether this code may read the rest.
#[derive(Deserialize)]
struct Header {
    schema: String,
    version: u32,
}

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
        if spec.fingerprint() != run.spec || score_fingerprint(source) != run.source {
            return Err(BundleError::NotThisRun);
        }
        let cells = run
            .cells
            .iter()
            .enumerate()
            .map(|(i, cell)| cell_v1(i, cell))
            .collect::<Result<_, _>>()?;
        Ok(Self {
            schema: BUNDLE_SCHEMA.to_owned(),
            version: BUNDLE_VERSION,
            spec: ExperimentSpecV1::from(spec),
            source: ScoreV1::from(source),
            identities: RunIdentitiesV1 {
                spec: run.spec,
                source: run.source,
                evaluation: run.evaluation,
            },
            population: run.corpus.as_ref().map(CorpusSnapshotV1::from),
            passes: run.passes.iter().map(GenerationPassV1::from).collect(),
            cells,
        })
    }

    /// The bundle as pretty, deterministic JSON.
    ///
    /// Serialising these types cannot fail: no map keys, and every custom
    /// serializer writes a string.
    #[must_use]
    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }

    /// Reads a bundle: parse, check schema and version, validate every
    /// projection, verify every identity. Never generates.
    ///
    /// # Errors
    /// The first [`BundleError`] found, in that order.
    pub fn from_json(json: &str) -> Result<Self, BundleError> {
        let header: Header =
            serde_json::from_str(json).map_err(|e| BundleError::Malformed(e.to_string()))?;
        if header.schema != BUNDLE_SCHEMA {
            return Err(BundleError::UnknownSchema(header.schema));
        }
        if header.version != BUNDLE_VERSION {
            return Err(BundleError::UnsupportedVersion(header.version));
        }
        let bundle: Self =
            serde_json::from_str(json).map_err(|e| BundleError::Malformed(e.to_string()))?;
        bundle.validate_projections()?;
        bundle.verify()?;
        Ok(bundle)
    }

    /// Every recorded model value can be held by the model.
    fn validate_projections(&self) -> Result<(), BundleError> {
        self.source.to_score().map_err(BundleError::Projection)?;
        self.spec.ask.to_ask().map_err(BundleError::Projection)?;
        if let EvaluationContextV1::GenerationAxes {
            pitch_material,
            references,
            ..
        } = &self.spec.evaluation
        {
            pitch_material
                .to_material()
                .map_err(BundleError::Projection)?;
            for reference in references {
                reference.to_score().map_err(BundleError::Projection)?;
            }
        }
        for cell in &self.cells {
            if let CellOutcomeV1::Produced(result) = &cell.outcome {
                result.score.to_score().map_err(BundleError::Projection)?;
            }
        }
        Ok(())
    }

    /// Recomputes every identity from the bundle's own data.
    ///
    /// # Errors
    /// [`BundleError::IdentityMismatch`] naming the first disagreement, in the
    /// order spec, source, evaluation, population, passes, cells.
    pub fn verify(&self) -> Result<(), BundleError> {
        let mismatch = |m| Err(BundleError::IdentityMismatch(m));
        if identity::spec_fingerprint(&self.spec) != self.identities.spec {
            return mismatch(Mismatch::Spec);
        }
        if self.source.fingerprint() != self.identities.source {
            return mismatch(Mismatch::Source);
        }
        if identity::evaluation_fingerprint(&self.spec.evaluation) != self.identities.evaluation {
            return mismatch(Mismatch::Evaluation);
        }
        let population = self.population.as_ref().map(|p| Channels {
            rhythms: p.rhythms,
            references: p.references,
            gesture: p.gesture,
        });
        if let (Some(p), Some(channels)) = (&self.population, population) {
            if identity::snapshot_whole(channels, &p.skipped) != p.whole {
                return mismatch(Mismatch::Population);
            }
        }
        let (source, ask) = (self.identities.source, self.spec.ask.fingerprint());
        for (i, pass) in self.passes.iter().enumerate() {
            let information = identity::pass_information(
                source,
                ask,
                (&pass.generator).into(),
                (&pass.scorer).into(),
                identity::offered(pass.regime.into(), population),
            );
            if information != pass.information {
                return mismatch(Mismatch::PassInformation { pass: i });
            }
        }
        for (i, cell) in self.cells.iter().enumerate() {
            self.verify_cell(i, cell, (source, ask))?;
        }
        Ok(())
    }

    fn verify_cell(
        &self,
        i: usize,
        cell: &CellV1,
        inputs: (Fingerprint, Fingerprint),
    ) -> Result<(), BundleError> {
        let mismatch = |m| Err(BundleError::IdentityMismatch(m));
        let Some(variant) = index(cell.variant).and_then(|v| self.spec.variants.get(v)) else {
            return mismatch(Mismatch::CellVariant { cell: i });
        };
        let Some(pass) = index(cell.pass).and_then(|p| self.passes.get(p)) else {
            return mismatch(Mismatch::CellPass { cell: i });
        };
        if pass.regime != cell.regime
            || pass.generator != variant.generator.identity
            || pass.scorer != variant.scorer.identity
        {
            return mismatch(Mismatch::CellPass { cell: i });
        }
        let requested = identity::cell_request(
            inputs,
            Stages {
                generator: (&variant.generator.identity).into(),
                scorer: (&variant.scorer.identity).into(),
                selector: (&variant.selector.identity).into(),
                realizer: (&variant.realizer.identity).into(),
            },
            cell.regime.into(),
            self.population.as_ref().map(|p| p.whole),
        );
        if requested != cell.requested {
            return mismatch(Mismatch::CellRequested { cell: i });
        }
        let recipe = identity::cell_recipe(
            pass.information,
            (&variant.selector.identity).into(),
            (&variant.realizer.identity).into(),
        );
        if recipe != cell.recipe {
            return mismatch(Mismatch::CellRecipe { cell: i });
        }
        if let CellOutcomeV1::Produced(result) = &cell.outcome {
            if result.score.fingerprint() != result.content {
                return mismatch(Mismatch::CellContent { cell: i });
            }
            for (m, metric) in result.metrics.iter().enumerate() {
                let expected = match metric.kind {
                    MetricKindV1::Evaluation => self.identities.evaluation,
                    MetricKindV1::PolicyObjective => Some(pass.information),
                };
                if expected != Some(metric.context) {
                    return mismatch(Mismatch::MetricContext { cell: i, metric: m });
                }
            }
        }
        Ok(())
    }

    /// The recorded spec as a runnable spec, when the current code still has
    /// every recorded stage identity.
    ///
    /// # Errors
    /// [`BundleError::IdentityDrift`], [`BundleError::Projection`],
    /// [`BundleError::Spec`].
    pub fn spec(&self) -> Result<ExperimentSpec, BundleError> {
        let variants = self
            .spec
            .variants
            .iter()
            .map(VariantSpecV1::to_variant)
            .collect::<Result<Vec<_>, _>>()?;
        let evaluation = match &self.spec.evaluation {
            EvaluationContextV1::None => EvaluationContext::None,
            EvaluationContextV1::GenerationAxes {
                evaluator,
                pitch_material,
                references,
            } => {
                undrifted(Stage::Evaluator, evaluator, EVALUATOR_GENERATION_AXES)?;
                EvaluationContext::GenerationAxes {
                    pitch_material: pitch_material
                        .to_material()
                        .map_err(BundleError::Projection)?,
                    references: references
                        .iter()
                        .map(ScoreV1::to_score)
                        .collect::<Result<_, _>>()
                        .map_err(BundleError::Projection)?,
                }
            }
        };
        let spec = ExperimentSpec {
            ask: self.spec.ask.to_ask().map_err(BundleError::Projection)?,
            variants,
            regimes: self.spec.regimes.iter().map(|&r| r.into()).collect(),
            evaluation,
        };
        spec.validate().map_err(BundleError::Spec)?;
        Ok(spec)
    }

    /// The recorded source score.
    ///
    /// # Errors
    /// [`BundleError::Projection`].
    pub fn source_score(&self) -> Result<Score, BundleError> {
        self.source.to_score().map_err(BundleError::Projection)
    }

    /// The in-memory run this bundle records, rebuilt exactly.
    ///
    /// # Errors
    /// [`BundleError::Projection`], [`BundleError::UnknownName`].
    pub fn run(&self) -> Result<ExperimentRun, BundleError> {
        Ok(ExperimentRun {
            spec: self.identities.spec,
            source: self.identities.source,
            corpus: self
                .population
                .as_ref()
                .map(CorpusSnapshotV1::to_snapshot)
                .transpose()?,
            evaluation: self.identities.evaluation,
            passes: self
                .passes
                .iter()
                .map(GenerationPassV1::to_pass)
                .collect::<Result<_, _>>()?,
            cells: self
                .cells
                .iter()
                .map(CellV1::to_cell)
                .collect::<Result<_, _>>()?,
        })
    }
}

// ── vocabulary ────────────────────────────────────────────────────────────────

/// Every policy id version 1 can record, taken from the policies themselves.
fn known_policy(recorded: &PolicyIdentityV1) -> Result<PolicyIdentity, BundleError> {
    [
        GeneratorPolicy::S6CandidateSet.identity(),
        ScorerPolicy::GenerationRerankV1.identity(),
        SelectorPolicy::IntactTop.identity(),
        SelectorPolicy::GlobalChainV1.identity(),
        RealizerPolicy::None.identity(),
        EVALUATOR_GENERATION_AXES,
    ]
    .iter()
    .find(|known| known.id == recorded.id)
    .map(|known| PolicyIdentity {
        id: known.id,
        version: recorded.version,
    })
    .ok_or_else(|| BundleError::UnknownName(recorded.id.clone()))
}

/// Every metric name version 1 can record.
fn known_metric(name: &str) -> Result<&'static str, BundleError> {
    RERANK_AXIS_LABELS
        .iter()
        .chain(&[METRIC_AGGREGATE, METRIC_CHAIN_COST])
        .find(|&&known| known == name)
        .copied()
        .ok_or_else(|| BundleError::UnknownName(name.to_owned()))
}

/// A recorded stage identity must be the one the current arm still has.
fn undrifted(
    stage: Stage,
    recorded: &PolicyIdentityV1,
    current: PolicyIdentity,
) -> Result<(), BundleError> {
    if PolicyRef::from(recorded) == PolicyRef::from(current) {
        Ok(())
    } else {
        Err(BundleError::IdentityDrift {
            stage,
            recorded: recorded.clone(),
        })
    }
}

fn wide(value: usize) -> u64 {
    u64::try_from(value).unwrap_or(u64::MAX)
}

fn index(value: u64) -> Option<usize> {
    usize::try_from(value).ok()
}

fn narrow(value: u64) -> Result<usize, BundleError> {
    count(value).map_err(BundleError::Projection)
}

// ── spec conversions ─────────────────────────────────────────────────────────

impl From<PolicyIdentity> for PolicyIdentityV1 {
    fn from(identity: PolicyIdentity) -> Self {
        Self {
            id: identity.id.to_owned(),
            version: identity.version,
        }
    }
}

impl From<InformationRegime> for InformationRegimeV1 {
    fn from(
        InformationRegime {
            rhythms,
            references,
            gesture,
        }: InformationRegime,
    ) -> Self {
        Self {
            rhythms,
            references,
            gesture,
        }
    }
}

impl From<InformationRegimeV1> for InformationRegime {
    fn from(
        InformationRegimeV1 {
            rhythms,
            references,
            gesture,
        }: InformationRegimeV1,
    ) -> Self {
        Self {
            rhythms,
            references,
            gesture,
        }
    }
}

impl From<&VariantSpec> for VariantSpecV1 {
    fn from(variant: &VariantSpec) -> Self {
        let VariantSpec {
            label,
            generator,
            scorer,
            selector,
            realizer,
        } = variant;
        Self {
            label: label.clone(),
            generator: StageV1 {
                policy: match generator {
                    GeneratorPolicy::S6CandidateSet => GeneratorPolicyV1::S6CandidateSet,
                },
                identity: generator.identity().into(),
            },
            scorer: StageV1 {
                policy: match scorer {
                    ScorerPolicy::GenerationRerankV1 => ScorerPolicyV1::GenerationRerankV1,
                },
                identity: scorer.identity().into(),
            },
            selector: StageV1 {
                policy: match selector {
                    SelectorPolicy::IntactTop => SelectorPolicyV1::IntactTop,
                    SelectorPolicy::GlobalChainV1 => SelectorPolicyV1::GlobalChainV1,
                },
                identity: selector.identity().into(),
            },
            realizer: StageV1 {
                policy: match realizer {
                    RealizerPolicy::None => RealizerPolicyV1::None,
                },
                identity: realizer.identity().into(),
            },
        }
    }
}

impl VariantSpecV1 {
    fn to_variant(&self) -> Result<VariantSpec, BundleError> {
        let generator = match self.generator.policy {
            GeneratorPolicyV1::S6CandidateSet => GeneratorPolicy::S6CandidateSet,
        };
        let scorer = match self.scorer.policy {
            ScorerPolicyV1::GenerationRerankV1 => ScorerPolicy::GenerationRerankV1,
        };
        let selector = match self.selector.policy {
            SelectorPolicyV1::IntactTop => SelectorPolicy::IntactTop,
            SelectorPolicyV1::GlobalChainV1 => SelectorPolicy::GlobalChainV1,
        };
        let realizer = match self.realizer.policy {
            RealizerPolicyV1::None => RealizerPolicy::None,
        };
        undrifted(
            Stage::Generator,
            &self.generator.identity,
            generator.identity(),
        )?;
        undrifted(Stage::Scorer, &self.scorer.identity, scorer.identity())?;
        undrifted(
            Stage::Selector,
            &self.selector.identity,
            selector.identity(),
        )?;
        undrifted(
            Stage::Realizer,
            &self.realizer.identity,
            realizer.identity(),
        )?;
        Ok(VariantSpec {
            label: self.label.clone(),
            generator,
            scorer,
            selector,
            realizer,
        })
    }
}

impl From<&EvaluationContext> for EvaluationContextV1 {
    fn from(context: &EvaluationContext) -> Self {
        match context {
            EvaluationContext::None => Self::None,
            EvaluationContext::GenerationAxes {
                pitch_material,
                references,
            } => Self::GenerationAxes {
                evaluator: EVALUATOR_GENERATION_AXES.into(),
                pitch_material: PitchMaterialV1::from(pitch_material),
                references: references.iter().map(ScoreV1::from).collect(),
            },
        }
    }
}

impl From<&ExperimentSpec> for ExperimentSpecV1 {
    fn from(spec: &ExperimentSpec) -> Self {
        let ExperimentSpec {
            ask,
            variants,
            regimes,
            evaluation,
        } = spec;
        Self {
            ask: GenerationAskV1::from(ask),
            variants: variants.iter().map(VariantSpecV1::from).collect(),
            regimes: regimes.iter().map(|&r| r.into()).collect(),
            evaluation: EvaluationContextV1::from(evaluation),
        }
    }
}

// ── run conversions ──────────────────────────────────────────────────────────

impl From<&CorpusSnapshot> for CorpusSnapshotV1 {
    fn from(snapshot: &CorpusSnapshot) -> Self {
        let CorpusSnapshot {
            rhythms,
            references,
            gesture,
            rhythm_count,
            reference_count,
            gesture_present,
            skipped,
            whole,
        } = snapshot;
        Self {
            rhythms: *rhythms,
            references: *references,
            gesture: *gesture,
            rhythm_count: wide(*rhythm_count),
            reference_count: wide(*reference_count),
            gesture_present: *gesture_present,
            skipped: skipped.clone(),
            whole: *whole,
        }
    }
}

impl CorpusSnapshotV1 {
    fn to_snapshot(&self) -> Result<CorpusSnapshot, BundleError> {
        Ok(CorpusSnapshot {
            rhythms: self.rhythms,
            references: self.references,
            gesture: self.gesture,
            rhythm_count: narrow(self.rhythm_count)?,
            reference_count: narrow(self.reference_count)?,
            gesture_present: self.gesture_present,
            skipped: self.skipped.clone(),
            whole: self.whole,
        })
    }
}

impl From<&GenerationPass> for GenerationPassV1 {
    fn from(pass: &GenerationPass) -> Self {
        let GenerationPass {
            regime,
            generator,
            scorer,
            information,
            contribution,
            candidates,
            candidate_count,
        } = *pass;
        let CorpusContribution {
            templates,
            references,
            gesture,
        } = contribution;
        Self {
            regime: regime.into(),
            generator: generator.into(),
            scorer: scorer.into(),
            information,
            contribution: CorpusContributionV1 {
                templates: wide(templates),
                references: wide(references),
                gesture,
            },
            candidates,
            candidate_count: wide(candidate_count),
        }
    }
}

impl GenerationPassV1 {
    fn to_pass(&self) -> Result<GenerationPass, BundleError> {
        Ok(GenerationPass {
            regime: self.regime.into(),
            generator: known_policy(&self.generator)?,
            scorer: known_policy(&self.scorer)?,
            information: self.information,
            contribution: CorpusContribution {
                templates: narrow(self.contribution.templates)?,
                references: narrow(self.contribution.references)?,
                gesture: self.contribution.gesture,
            },
            candidates: self.candidates,
            candidate_count: narrow(self.candidate_count)?,
        })
    }
}

fn cell_v1(i: usize, cell: &Cell) -> Result<CellV1, BundleError> {
    let Cell {
        variant,
        regime,
        pass,
        requested,
        recipe,
        outcome,
    } = cell;
    let outcome = match outcome {
        CellOutcome::Produced(result) => {
            let ExperimentResult {
                score,
                content,
                realization,
                metrics,
                diagnostics,
            } = result.as_ref();
            let metrics = metrics
                .iter()
                .enumerate()
                .map(|(m, metric)| {
                    if metric.value.is_finite() {
                        Ok(MetricValueV1::from(metric))
                    } else {
                        Err(BundleError::NonFiniteMetric { cell: i, metric: m })
                    }
                })
                .collect::<Result<_, _>>()?;
            CellOutcomeV1::Produced(Box::new(ExperimentResultV1 {
                score: ScoreV1::from(score),
                content: *content,
                realization: realization.map(|never| match never {}),
                metrics,
                diagnostics: diagnostics.iter().map(|&d| d.into()).collect(),
            }))
        }
        CellOutcome::Refused(refusal) => CellOutcomeV1::Refused((*refusal).into()),
    };
    Ok(CellV1 {
        variant: wide(*variant),
        regime: (*regime).into(),
        pass: wide(*pass),
        requested: *requested,
        recipe: *recipe,
        outcome,
    })
}

impl CellV1 {
    fn to_cell(&self) -> Result<Cell, BundleError> {
        let outcome = match &self.outcome {
            CellOutcomeV1::Produced(result) => {
                let ExperimentResultV1 {
                    score,
                    content,
                    realization,
                    metrics,
                    diagnostics,
                } = result.as_ref();
                CellOutcome::Produced(Box::new(ExperimentResult {
                    score: score.to_score().map_err(BundleError::Projection)?,
                    content: *content,
                    realization: realization.map(|never| match never {}),
                    metrics: metrics
                        .iter()
                        .map(MetricValueV1::to_metric)
                        .collect::<Result<_, _>>()?,
                    diagnostics: diagnostics
                        .iter()
                        .map(|&d| Diagnostic::try_from(d))
                        .collect::<Result<_, _>>()?,
                }))
            }
            CellOutcomeV1::Refused(refusal) => CellOutcome::Refused((*refusal).try_into()?),
        };
        Ok(Cell {
            variant: narrow(self.variant)?,
            regime: self.regime.into(),
            pass: narrow(self.pass)?,
            requested: self.requested,
            recipe: self.recipe,
            outcome,
        })
    }
}

impl From<&MetricValue> for MetricValueV1 {
    fn from(metric: &MetricValue) -> Self {
        let MetricValue { identity, value } = *metric;
        let MetricIdentity {
            kind,
            name,
            owner,
            context,
        } = identity;
        Self {
            kind: match kind {
                MetricKind::Evaluation => MetricKindV1::Evaluation,
                MetricKind::PolicyObjective => MetricKindV1::PolicyObjective,
            },
            name: name.to_owned(),
            owner: owner.into(),
            context,
            value,
        }
    }
}

impl MetricValueV1 {
    fn to_metric(&self) -> Result<MetricValue, BundleError> {
        Ok(MetricValue {
            identity: MetricIdentity {
                kind: match self.kind {
                    MetricKindV1::Evaluation => MetricKind::Evaluation,
                    MetricKindV1::PolicyObjective => MetricKind::PolicyObjective,
                },
                name: known_metric(&self.name)?,
                owner: known_policy(&self.owner)?,
                context: self.context,
            },
            value: self.value,
        })
    }
}

impl From<GenerationStrategy> for StrategyV1 {
    fn from(strategy: GenerationStrategy) -> Self {
        match strategy {
            GenerationStrategy::RhythmCopyPitchSubstitute => Self::RhythmCopyPitchSubstitute,
            GenerationStrategy::MotifTransposeVariation => Self::MotifTransposeVariation,
            GenerationStrategy::ConstrainedRandomWalk => Self::ConstrainedRandomWalk,
            GenerationStrategy::ShuffleMotifs => Self::ShuffleMotifs,
            GenerationStrategy::RepeatVariation => Self::RepeatVariation,
        }
    }
}

impl From<StrategyV1> for GenerationStrategy {
    fn from(strategy: StrategyV1) -> Self {
        match strategy {
            StrategyV1::RhythmCopyPitchSubstitute => Self::RhythmCopyPitchSubstitute,
            StrategyV1::MotifTransposeVariation => Self::MotifTransposeVariation,
            StrategyV1::ConstrainedRandomWalk => Self::ConstrainedRandomWalk,
            StrategyV1::ShuffleMotifs => Self::ShuffleMotifs,
            StrategyV1::RepeatVariation => Self::RepeatVariation,
        }
    }
}

impl From<Diagnostic> for DiagnosticV1 {
    fn from(diagnostic: Diagnostic) -> Self {
        match diagnostic {
            Diagnostic::Selected {
                candidate,
                rank,
                strategy,
                variant_seed,
            } => Self::Selected {
                candidate: wide(candidate),
                rank: wide(rank),
                strategy: strategy.into(),
                variant_seed,
            },
            Diagnostic::ChainBar {
                bar,
                candidate,
                rank,
                strategy,
                variant_seed,
            } => Self::ChainBar {
                bar: wide(bar),
                candidate: wide(candidate),
                rank: wide(rank),
                strategy: strategy.into(),
                variant_seed,
            },
        }
    }
}

impl TryFrom<DiagnosticV1> for Diagnostic {
    type Error = BundleError;

    fn try_from(diagnostic: DiagnosticV1) -> Result<Self, BundleError> {
        Ok(match diagnostic {
            DiagnosticV1::Selected {
                candidate,
                rank,
                strategy,
                variant_seed,
            } => Self::Selected {
                candidate: narrow(candidate)?,
                rank: narrow(rank)?,
                strategy: strategy.into(),
                variant_seed,
            },
            DiagnosticV1::ChainBar {
                bar,
                candidate,
                rank,
                strategy,
                variant_seed,
            } => Self::ChainBar {
                bar: narrow(bar)?,
                candidate: narrow(candidate)?,
                rank: narrow(rank)?,
                strategy: strategy.into(),
                variant_seed,
            },
        })
    }
}

// ── refusal conversions ──────────────────────────────────────────────────────

impl From<CellRefusal> for CellRefusalV1 {
    fn from(refusal: CellRefusal) -> Self {
        match refusal {
            CellRefusal::EmptySet => Self::EmptySet,
            CellRefusal::Chain(error) => Self::Chain(error.into()),
        }
    }
}

impl TryFrom<CellRefusalV1> for CellRefusal {
    type Error = BundleError;

    fn try_from(refusal: CellRefusalV1) -> Result<Self, BundleError> {
        Ok(match refusal {
            CellRefusalV1::EmptySet => Self::EmptySet,
            CellRefusalV1::Chain(error) => Self::Chain(error.try_into()?),
        })
    }
}

impl From<StateId> for StateIdV1 {
    fn from(StateId { layer, ordinal }: StateId) -> Self {
        Self {
            layer: wide(layer),
            ordinal: wide(ordinal),
        }
    }
}

impl TryFrom<StateIdV1> for StateId {
    type Error = BundleError;

    fn try_from(StateIdV1 { layer, ordinal }: StateIdV1) -> Result<Self, BundleError> {
        Ok(Self {
            layer: narrow(layer)?,
            ordinal: narrow(ordinal)?,
        })
    }
}

impl From<EdgeId> for EdgeIdV1 {
    fn from(EdgeId { from, to }: EdgeId) -> Self {
        Self {
            from: from.into(),
            to: to.into(),
        }
    }
}

impl TryFrom<EdgeIdV1> for EdgeId {
    type Error = BundleError;

    fn try_from(EdgeIdV1 { from, to }: EdgeIdV1) -> Result<Self, BundleError> {
        Ok(Self {
            from: from.try_into()?,
            to: to.try_into()?,
        })
    }
}

impl From<PathError> for PathErrorV1 {
    fn from(error: PathError) -> Self {
        match error {
            PathError::NoLayers => Self::NoLayers,
            PathError::EmptyLayer { layer } => Self::EmptyLayer { layer: wide(layer) },
            PathError::TransitionCount { expected, found } => Self::TransitionCount {
                expected: wide(expected),
                found: wide(found),
            },
            PathError::TransitionShape {
                layer,
                expected,
                found,
            } => Self::TransitionShape {
                layer: wide(layer),
                expected: (wide(expected.0), wide(expected.1)),
                found: (wide(found.0), wide(found.1)),
            },
            PathError::NonFiniteLocal { state, cost } => Self::NonFiniteLocal {
                state: state.into(),
                cost_bits: cost.to_bits(),
            },
            PathError::NonFiniteTransition { edge, cost } => Self::NonFiniteTransition {
                edge: edge.into(),
                cost_bits: cost.to_bits(),
            },
            PathError::NonFiniteAccumulation { state, cost } => Self::NonFiniteAccumulation {
                state: state.into(),
                cost_bits: cost.to_bits(),
            },
            PathError::KZero => Self::KZero,
            PathError::MinDistanceZero => Self::MinDistanceZero,
            PathError::MinDistanceUnsatisfiable {
                min_distance,
                layers,
            } => Self::MinDistanceUnsatisfiable {
                min_distance: wide(min_distance),
                layers: wide(layers),
            },
        }
    }
}

impl TryFrom<PathErrorV1> for PathError {
    type Error = BundleError;

    fn try_from(error: PathErrorV1) -> Result<Self, BundleError> {
        Ok(match error {
            PathErrorV1::NoLayers => Self::NoLayers,
            PathErrorV1::EmptyLayer { layer } => Self::EmptyLayer {
                layer: narrow(layer)?,
            },
            PathErrorV1::TransitionCount { expected, found } => Self::TransitionCount {
                expected: narrow(expected)?,
                found: narrow(found)?,
            },
            PathErrorV1::TransitionShape {
                layer,
                expected,
                found,
            } => Self::TransitionShape {
                layer: narrow(layer)?,
                expected: (narrow(expected.0)?, narrow(expected.1)?),
                found: (narrow(found.0)?, narrow(found.1)?),
            },
            PathErrorV1::NonFiniteLocal { state, cost_bits } => Self::NonFiniteLocal {
                state: state.try_into()?,
                cost: f64::from_bits(cost_bits),
            },
            PathErrorV1::NonFiniteTransition { edge, cost_bits } => Self::NonFiniteTransition {
                edge: edge.try_into()?,
                cost: f64::from_bits(cost_bits),
            },
            PathErrorV1::NonFiniteAccumulation { state, cost_bits } => {
                Self::NonFiniteAccumulation {
                    state: state.try_into()?,
                    cost: f64::from_bits(cost_bits),
                }
            }
            PathErrorV1::KZero => Self::KZero,
            PathErrorV1::MinDistanceZero => Self::MinDistanceZero,
            PathErrorV1::MinDistanceUnsatisfiable {
                min_distance,
                layers,
            } => Self::MinDistanceUnsatisfiable {
                min_distance: narrow(min_distance)?,
                layers: narrow(layers)?,
            },
        })
    }
}

impl From<ChainError> for ChainErrorV1 {
    #[allow(clippy::too_many_lines)] // one arm per variant, mirrored
    fn from(error: ChainError) -> Self {
        match error {
            ChainError::EmptySet => Self::EmptySet,
            ChainError::NoBars => Self::NoBars,
            ChainError::BarCountMismatch {
                candidate,
                expected,
                found,
            } => Self::BarCountMismatch {
                candidate: wide(candidate),
                expected: wide(expected),
                found: wide(found),
            },
            ChainError::PpqMismatch {
                candidate,
                expected,
                found,
            } => Self::PpqMismatch {
                candidate: wide(candidate),
                expected,
                found,
            },
            ChainError::MasterBarMismatch {
                candidate,
                bar,
                field,
            } => Self::MasterBarMismatch {
                candidate: wide(candidate),
                bar: wide(bar),
                field: match field {
                    MasterBarField::Index => MasterBarFieldV1::Index,
                    MasterBarField::TickRange => MasterBarFieldV1::TickRange,
                    MasterBarField::TimeSignature => MasterBarFieldV1::TimeSignature,
                    MasterBarField::Tempo => MasterBarFieldV1::Tempo,
                    MasterBarField::Repeat => MasterBarFieldV1::Repeat,
                },
            },
            ChainError::TrackCountMismatch {
                candidate,
                expected,
                found,
            } => Self::TrackCountMismatch {
                candidate: wide(candidate),
                expected: wide(expected),
                found: wide(found),
            },
            ChainError::TrackMetadataMismatch {
                candidate,
                track,
                field,
            } => Self::TrackMetadataMismatch {
                candidate: wide(candidate),
                track: wide(track),
                field: match field {
                    TrackField::Name => TrackFieldV1::Name,
                    TrackField::Channel => TrackFieldV1::Channel,
                    TrackField::Tuning => TrackFieldV1::Tuning,
                    TrackField::VoiceCount => TrackFieldV1::VoiceCount,
                    TrackField::VoiceId => TrackFieldV1::VoiceId,
                },
            },
            ChainError::SourceMetaMismatch { candidate } => Self::SourceMetaMismatch {
                candidate: wide(candidate),
            },
            ChainError::LossReportMismatch { candidate } => Self::LossReportMismatch {
                candidate: wide(candidate),
            },
            ChainError::CrossBarMaterial { candidate, bar } => Self::CrossBarMaterial {
                candidate: wide(candidate),
                bar: wide(bar),
            },
            ChainError::EmptyEventGroup { candidate } => Self::EmptyEventGroup {
                candidate: wide(candidate),
            },
            ChainError::MaterialOutsideTimeline { candidate, tick } => {
                Self::MaterialOutsideTimeline {
                    candidate: wide(candidate),
                    tick,
                }
            }
            ChainError::MissingMaterial {
                candidate,
                track,
                voice,
                bar,
            } => Self::MissingMaterial {
                candidate: wide(candidate),
                track: wide(track),
                voice: wide(voice),
                bar: wide(bar),
            },
            ChainError::BoundaryFact(fact) => Self::BoundaryFact(match fact {
                TransitionFactError::MissingFromBar { bar, bars } => {
                    TransitionFactErrorV1::MissingFromBar {
                        bar: wide(bar),
                        bars: wide(bars),
                    }
                }
                TransitionFactError::MissingToBar { bar, bars } => {
                    TransitionFactErrorV1::MissingToBar {
                        bar: wide(bar),
                        bars: wide(bars),
                    }
                }
            }),
            ChainError::Path(path) => Self::Path(path.into()),
        }
    }
}

impl TryFrom<ChainErrorV1> for ChainError {
    type Error = BundleError;

    #[allow(clippy::too_many_lines)] // one arm per variant, mirrored
    fn try_from(error: ChainErrorV1) -> Result<Self, BundleError> {
        Ok(match error {
            ChainErrorV1::EmptySet => Self::EmptySet,
            ChainErrorV1::NoBars => Self::NoBars,
            ChainErrorV1::BarCountMismatch {
                candidate,
                expected,
                found,
            } => Self::BarCountMismatch {
                candidate: narrow(candidate)?,
                expected: narrow(expected)?,
                found: narrow(found)?,
            },
            ChainErrorV1::PpqMismatch {
                candidate,
                expected,
                found,
            } => Self::PpqMismatch {
                candidate: narrow(candidate)?,
                expected,
                found,
            },
            ChainErrorV1::MasterBarMismatch {
                candidate,
                bar,
                field,
            } => Self::MasterBarMismatch {
                candidate: narrow(candidate)?,
                bar: narrow(bar)?,
                field: match field {
                    MasterBarFieldV1::Index => MasterBarField::Index,
                    MasterBarFieldV1::TickRange => MasterBarField::TickRange,
                    MasterBarFieldV1::TimeSignature => MasterBarField::TimeSignature,
                    MasterBarFieldV1::Tempo => MasterBarField::Tempo,
                    MasterBarFieldV1::Repeat => MasterBarField::Repeat,
                },
            },
            ChainErrorV1::TrackCountMismatch {
                candidate,
                expected,
                found,
            } => Self::TrackCountMismatch {
                candidate: narrow(candidate)?,
                expected: narrow(expected)?,
                found: narrow(found)?,
            },
            ChainErrorV1::TrackMetadataMismatch {
                candidate,
                track,
                field,
            } => Self::TrackMetadataMismatch {
                candidate: narrow(candidate)?,
                track: narrow(track)?,
                field: match field {
                    TrackFieldV1::Name => TrackField::Name,
                    TrackFieldV1::Channel => TrackField::Channel,
                    TrackFieldV1::Tuning => TrackField::Tuning,
                    TrackFieldV1::VoiceCount => TrackField::VoiceCount,
                    TrackFieldV1::VoiceId => TrackField::VoiceId,
                },
            },
            ChainErrorV1::SourceMetaMismatch { candidate } => Self::SourceMetaMismatch {
                candidate: narrow(candidate)?,
            },
            ChainErrorV1::LossReportMismatch { candidate } => Self::LossReportMismatch {
                candidate: narrow(candidate)?,
            },
            ChainErrorV1::CrossBarMaterial { candidate, bar } => Self::CrossBarMaterial {
                candidate: narrow(candidate)?,
                bar: narrow(bar)?,
            },
            ChainErrorV1::EmptyEventGroup { candidate } => Self::EmptyEventGroup {
                candidate: narrow(candidate)?,
            },
            ChainErrorV1::MaterialOutsideTimeline { candidate, tick } => {
                Self::MaterialOutsideTimeline {
                    candidate: narrow(candidate)?,
                    tick,
                }
            }
            ChainErrorV1::MissingMaterial {
                candidate,
                track,
                voice,
                bar,
            } => Self::MissingMaterial {
                candidate: narrow(candidate)?,
                track: narrow(track)?,
                voice: narrow(voice)?,
                bar: narrow(bar)?,
            },
            ChainErrorV1::BoundaryFact(fact) => Self::BoundaryFact(match fact {
                TransitionFactErrorV1::MissingFromBar { bar, bars } => {
                    TransitionFactError::MissingFromBar {
                        bar: narrow(bar)?,
                        bars: narrow(bars)?,
                    }
                }
                TransitionFactErrorV1::MissingToBar { bar, bars } => {
                    TransitionFactError::MissingToBar {
                        bar: narrow(bar)?,
                        bars: narrow(bars)?,
                    }
                }
            }),
            ChainErrorV1::Path(path) => Self::Path(path.try_into()?),
        })
    }
}
