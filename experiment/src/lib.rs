//! Generator Observatory: algorithm variant × information regime as two
//! independent, reproducible experiment axes over the shared generation path.
//!
//! One [`ExperimentSpec`] fixes the ask, the variants, the information regimes
//! and the evaluation context; [`run_experiment`] runs every cell over one
//! source and one bound corpus population, headlessly, through the same
//! `griff_core::generation_input` entry the CLI and the cockpit use.
//!
//! What this crate owns, and what it deliberately does not:
//!
//! - **Information regime** ([`InformationRegime`]) masks the channels of an
//!   *already prepared* corpus — rhythm templates, novelty references, gesture.
//!   Which songs a population holds, holdout, and splits are the offline
//!   Reachability Lab's (ADR-0032); nothing here selects or filters a
//!   population, and nothing here calls one a valid holdout.
//! - **Identities are separate** — the experiment spec, the bound corpus
//!   population ([`CorpusSnapshot`]), the information a cell could actually
//!   consume (its pass), and the evaluation context never collapse into one.
//! - **Metrics carry a comparability identity** ([`MetricIdentity`]). A delta
//!   or an interaction exists only between compatible identities; otherwise it
//!   is [`Comparison::Unavailable`], never a number.
//! - **Results are more than a score**: [`ExperimentResult::realization`] is a
//!   typed extension point that no current policy fills.
//!
//! Pure and deterministic: no I/O, no clock, no hash-map order. A run is
//! persisted as an [`ExperimentBundleV1`] — the canonical projection written
//! down, verified on load, and never regenerated.

mod bundle;
mod fingerprint;
mod metric;
mod projection;
mod regime;
mod run;
mod spec;

pub use bundle::{
    BundleError, CellOutcomeV1, CellRefusalV1, CellV1, ChainErrorV1, CorpusContributionV1,
    CorpusSnapshotV1, DiagnosticV1, EdgeIdV1, EvaluationContextV1, ExperimentBundleV1,
    ExperimentResultV1, ExperimentSpecV1, GenerationPassV1, GeneratorPolicyV1, InformationRegimeV1,
    MasterBarFieldV1, MetricKindV1, MetricValueV1, Mismatch, PathErrorV1, PolicyIdentityV1,
    RealizationV1, RealizerPolicyV1, RunIdentitiesV1, ScorerPolicyV1, SelectorPolicyV1, Stage,
    StageV1, StateIdV1, StrategyV1, TrackFieldV1, TransitionFactErrorV1, VariantSpecV1,
    BUNDLE_SCHEMA, BUNDLE_VERSION,
};
pub use fingerprint::{
    ask_fingerprint, gesture_fingerprint, references_fingerprint, rhythms_fingerprint,
    score_fingerprint, Fingerprint,
};
pub use metric::{
    delta, interaction, Comparison, MetricIdentity, MetricKind, MetricValue, Unavailable,
    EVALUATOR_GENERATION_AXES,
};
pub use projection::{
    AtomV1, EventGroupKindV1, EventGroupV1, EvidenceV1, GenerationAskV1, GestureControlV1,
    ImportWarningV1, MarksV1, MasterBarV1, NotePositionV1, NoteV1, PitchMaterialV1,
    ProjectionError, RestV1, RhythmTemplateV1, ScoreV1, SourceMetaV1, SpanTechniqueV1,
    TechniqueSourceV1, TechniqueSpanV1, TemplateNoteV1, TickRangeV1, TrackV1, VoiceV1,
};
pub use regime::InformationRegime;
pub use run::{
    corpus_snapshot, run_experiment, Cell, CellOutcome, CellRefusal, CorpusSnapshot, Diagnostic,
    ExperimentInputs, ExperimentResult, ExperimentRun, GenerationPass, RealizationArtifact,
    RunError, METRIC_AGGREGATE, METRIC_CHAIN_COST,
};
pub use spec::{
    EvaluationContext, ExperimentSpec, GeneratorPolicy, PolicyIdentity, RealizerPolicy,
    ScorerPolicy, SelectorPolicy, SpecError, VariantSpec,
};
