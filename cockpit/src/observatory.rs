//! The cockpit's Generator Observatory panel (ADR-0034, S8).
//!
//! **The panel shows bundles, never runs.** Its only experiment state is a
//! [`LoadedExperiment`]: an [`ExperimentBundleV1`] and the [`ExperimentView`]
//! arranged from it. A native Run materialises the run as a bundle before
//! anything is shown and drops the run; Open reads a saved bundle through the
//! same verification. Both reach the screen through one display path, and
//! selecting, auditioning, or A/B-switching cells reads recorded scores — there
//! is nothing here that could generate, rerank, or plan.
//!
//! The words a person reads are built here, at render time, from the typed
//! view: a comparison the experiment API calls unavailable is said to be not
//! comparable, never shown as a number.

use griff_core::generation_input::{
    generation_request_from_score, CorpusContribution, CorpusMaterial, GenerationAsk,
};
use griff_core::score::Score;
use griff_experiment::{
    run_experiment, BundleError, CellRefusal, Comparison, Diagnostic, EvaluationContext,
    ExperimentBundleV1, ExperimentInputs, ExperimentSpec, Fingerprint, InformationRegime, RunError,
    SpecError, Unavailable, VariantSpec,
};
use griff_ui_core::observatory::{ExperimentView, RegimeName};

/// Where a shown experiment came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Origin {
    /// Run in this session, then written down.
    Run,
    /// Opened from a saved bundle.
    File(String),
}

/// An experiment the panel can show: a verified bundle and its view.
#[derive(Debug, Clone)]
pub struct LoadedExperiment {
    /// The bundle — the only source of what is shown.
    pub bundle: ExperimentBundleV1,
    /// The bundle, arranged for display.
    pub view: ExperimentView,
    /// Where it came from.
    pub origin: Origin,
}

/// Why a Run produced nothing to show.
#[derive(Debug)]
pub enum RunFailure {
    /// The runner refused (an invalid spec, a source that cannot seed).
    Run(RunError),
    /// The run could not be written down or arranged.
    Bundle(BundleError),
}

impl LoadedExperiment {
    /// Arranges an already-verified `bundle`.
    ///
    /// # Errors
    /// Whatever arranging the bundle refuses.
    pub fn from_bundle(bundle: ExperimentBundleV1, origin: Origin) -> Result<Self, BundleError> {
        let view = ExperimentView::from_bundle(&bundle)?;
        Ok(Self {
            bundle,
            view,
            origin,
        })
    }

    /// Reads, verifies, and arranges a saved bundle.
    ///
    /// # Errors
    /// Whatever loading or arranging refuses.
    pub fn open_json(json: &str, origin: Origin) -> Result<Self, BundleError> {
        Self::from_bundle(ExperimentBundleV1::from_json(json)?, origin)
    }

    /// Runs `spec`, writes the run down as a bundle, and arranges the bundle.
    /// The run itself never leaves this function.
    ///
    /// # Errors
    /// [`RunFailure`].
    pub fn run(
        spec: &ExperimentSpec,
        source: &Score,
        corpus: Option<&CorpusMaterial>,
    ) -> Result<Self, RunFailure> {
        let run =
            run_experiment(spec, &ExperimentInputs { source, corpus }).map_err(RunFailure::Run)?;
        let bundle =
            ExperimentBundleV1::from_run(spec, source, &run).map_err(RunFailure::Bundle)?;
        Self::from_bundle(bundle, Origin::Run).map_err(RunFailure::Bundle)
    }
}

/// The milestone-1 experiment over `source`: S6 Intact and S7 Global Chain
/// under seed-only and full, with the ask the Generate knobs describe.
///
/// The evaluation context is explicit: none unless the user asks to evaluate
/// against the source (its own scale, and itself as the only reference).
///
/// # Errors
/// A message when `source` cannot seed the evaluation context.
pub fn milestone_spec(
    ask: GenerationAsk,
    source: &Score,
    evaluate_against_source: bool,
) -> Result<ExperimentSpec, String> {
    let evaluation = if evaluate_against_source {
        let base = generation_request_from_score(source, ask.seed, ask.bars)
            .map_err(|err| format!("the source cannot seed an evaluation context: {err:?}"))?;
        EvaluationContext::GenerationAxes {
            pitch_material: base.pitch_material,
            references: vec![source.clone()],
        }
    } else {
        EvaluationContext::None
    };
    Ok(ExperimentSpec {
        ask,
        variants: vec![VariantSpec::s6_intact(), VariantSpec::s7_global_chain()],
        regimes: vec![InformationRegime::SEED_ONLY, InformationRegime::FULL],
        evaluation,
    })
}

/// The Observatory panel's state.
#[derive(Debug, Default)]
pub struct ObservatoryPanel {
    /// Whether the window is shown (the `o` key toggles it).
    pub open: bool,
    /// Whether Run supplies an evaluation context (against the source).
    pub evaluate_against_source: bool,
    /// The shown experiment, if any.
    pub loaded: Option<LoadedExperiment>,
    /// Cell A (index into the view's cells).
    pub a: Option<usize>,
    /// Cell B.
    pub b: Option<usize>,
    /// The bundle path the native Open reads and Save writes beside.
    pub path: String,
    /// Outcome of the last action.
    pub status: Option<String>,
}

impl ObservatoryPanel {
    /// Shows `loaded`: A is the first variant under the first regime, B the
    /// last variant under the last regime.
    pub fn install(&mut self, loaded: LoadedExperiment) {
        let view = &loaded.view;
        self.a = view.cell_index(0, 0);
        self.b = view.cell_index(
            view.variants.len().saturating_sub(1),
            view.regimes.len().saturating_sub(1),
        );
        self.loaded = Some(loaded);
        self.open = true;
    }
}

/// What the Observatory window asked for, applied once the window no longer
/// borrows the panel.
#[derive(Debug, Default, Clone, Copy)]
#[allow(clippy::struct_excessive_bools)] // one flag per button
pub struct Actions {
    /// Run the milestone experiment (native).
    pub run: bool,
    /// Open the bundle at the panel's path (native).
    pub open: bool,
    /// Save the shown bundle (native).
    pub save: bool,
    /// Make this cell A.
    pub select_a: Option<usize>,
    /// Make this cell B.
    pub select_b: Option<usize>,
    /// Audition this cell.
    pub play: Option<usize>,
    /// Swap to the other of the last two auditions.
    pub ab: bool,
}

/// The first twelve hex digits of a fingerprint, for a label.
#[must_use]
pub fn short(fingerprint: Fingerprint) -> String {
    fingerprint.to_hex().chars().take(12).collect()
}

/// A comparison, in words: the signed number when the experiment API gives
/// one, and otherwise why there is none.
#[must_use]
pub fn comparison_text(comparison: Comparison) -> String {
    match comparison {
        Comparison::Available(value) => format!("{value:+.3}"),
        Comparison::Unavailable(Unavailable::IncompatibleIdentity) => {
            "not comparable: different measurement context".to_owned()
        }
        Comparison::Unavailable(Unavailable::NotAnEvaluation) => {
            "not comparable: not an evaluation".to_owned()
        }
        Comparison::Unavailable(Unavailable::Missing) => "not measured on both".to_owned(),
    }
}

/// A requested regime, in words.
#[must_use]
pub fn regime_text(name: RegimeName, channels: InformationRegime) -> String {
    match name {
        RegimeName::SeedOnly => "seed only".to_owned(),
        RegimeName::RhythmsOnly => "rhythms only".to_owned(),
        RegimeName::ReferencesOnly => "references only".to_owned(),
        RegimeName::GestureOnly => "gesture only".to_owned(),
        RegimeName::Full => "full (rhythms + references + gesture)".to_owned(),
        RegimeName::Custom => [
            (channels.rhythms, "rhythms"),
            (channels.references, "references"),
            (channels.gesture, "gesture"),
        ]
        .iter()
        .filter(|(open, _)| *open)
        .map(|(_, channel)| *channel)
        .collect::<Vec<_>>()
        .join(" + "),
    }
}

/// What a corpus actually contributed, in words.
#[must_use]
pub fn contribution_text(contribution: CorpusContribution) -> String {
    if contribution.is_seed_only() {
        return "nothing (seed only)".to_owned();
    }
    format!(
        "{} rhythm templates · {} references · {}",
        contribution.templates,
        contribution.references,
        if contribution.gesture {
            "gesture carved"
        } else {
            "no gesture"
        }
    )
}

/// A refusal, in words.
#[must_use]
pub fn refusal_text(refusal: CellRefusal) -> String {
    match refusal {
        CellRefusal::EmptySet => "refused: empty candidate set".to_owned(),
        CellRefusal::Chain(error) => format!("refused: {}", crate::chain_refusal_summary(error)),
    }
}

/// How a result was selected, in words.
#[must_use]
pub fn diagnostic_text(diagnostic: Diagnostic) -> String {
    match diagnostic {
        Diagnostic::Selected {
            rank,
            strategy,
            variant_seed,
            ..
        } => format!("took rank {rank} whole ({strategy:?}, seed {variant_seed:016x})"),
        Diagnostic::ChainBar {
            bar,
            rank,
            strategy,
            ..
        } => format!(
            "bar {} from rank {rank} ({strategy:?})",
            bar.saturating_add(1)
        ),
    }
}

/// A cell's label: its variant and requested regime.
#[must_use]
pub fn cell_title(view: &ExperimentView, cell: usize) -> String {
    view.cells.get(cell).map_or_else(String::new, |c| {
        let variant = view
            .variants
            .get(c.variant)
            .map_or("?", |v| v.label.as_str());
        let regime = view
            .regimes
            .get(c.regime)
            .map_or_else(|| "?".to_owned(), |r| regime_text(r.name, r.channels));
        format!("{variant} · {regime}")
    })
}

/// Why a bundle could not be opened, in words.
#[must_use]
pub fn bundle_error_text(error: &BundleError) -> String {
    match error {
        BundleError::Malformed(message) => format!("not a readable bundle ({message})"),
        BundleError::Serialize(message) => format!("could not be written ({message})"),
        BundleError::UnknownSchema(schema) => format!("not an experiment bundle ({schema})"),
        BundleError::UnsupportedVersion(version) => {
            format!("bundle version {version} is not supported")
        }
        BundleError::Projection(projection) => {
            format!("records a value the model cannot hold ({projection:?})")
        }
        BundleError::IdentityMismatch(mismatch) => {
            format!("its data does not match its recorded identities ({mismatch:?})")
        }
        BundleError::UnknownName(name) => format!("records an unknown name ({name})"),
        BundleError::IdentityDrift { stage, recorded } => format!(
            "its {stage:?} {} v{} is not this code's",
            recorded.id, recorded.version
        ),
        BundleError::Spec(spec) => format!("records an invalid spec ({spec:?})"),
        BundleError::NotThisRun => "the spec or source is not this run's".to_owned(),
        BundleError::NonFiniteMetric { cell, metric } => {
            format!("cell {cell} metric {metric} is not a finite number")
        }
    }
}

/// Why a Run produced nothing, in words.
#[must_use]
pub fn failure_text(failure: &RunFailure) -> String {
    match failure {
        RunFailure::Run(RunError::Spec(SpecError::GestureChannelDeclinedByAsk(_))) => {
            "the full regime opens the gesture channel — turn gesture on in Generate".to_owned()
        }
        RunFailure::Run(RunError::Spec(spec)) => format!("invalid experiment ({spec:?})"),
        RunFailure::Run(RunError::Generation(generation)) => {
            format!("the source cannot seed a generation pass ({generation:?})")
        }
        RunFailure::Bundle(error) => bundle_error_text(error),
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic, clippy::indexing_slicing)]

    use super::*;
    use griff_core::import::import_score_auto;
    use griff_experiment::Unavailable;

    fn demo() -> Score {
        import_score_auto(include_bytes!("../assets/demo.mid")).expect("demo imports")
    }

    fn ask() -> GenerationAsk {
        GenerationAsk {
            seed: 7,
            bars: 4,
            variants_per_strategy: 2,
            gesture: true,
            tonal: None,
        }
    }

    #[test]
    fn a_run_reaches_the_panel_only_as_its_bundle() {
        let source = demo();
        let spec = milestone_spec(ask(), &source, true).expect("the demo seeds");
        let fresh = LoadedExperiment::run(&spec, &source, None).expect("runs");
        assert_eq!(fresh.origin, Origin::Run);
        assert_eq!(
            Ok(fresh.view.clone()),
            ExperimentView::from_bundle(&fresh.bundle),
            "what is shown is the bundle arranged"
        );
        let reopened = LoadedExperiment::open_json(
            &fresh.bundle.to_json().expect("serializes"),
            Origin::File("saved.json".to_owned()),
        )
        .expect("loads");
        assert_eq!(
            reopened.view, fresh.view,
            "a fresh run and its saved bundle show the same thing"
        );
    }

    #[test]
    fn the_milestone_spec_is_s6_and_s7_under_seed_only_and_full() {
        let source = demo();
        let spec = milestone_spec(ask(), &source, false).expect("seeds");
        assert_eq!(
            spec.variants
                .iter()
                .map(|v| v.label.as_str())
                .collect::<Vec<_>>(),
            ["S6 Intact", "S7 Global Chain"]
        );
        assert_eq!(
            spec.regimes,
            [InformationRegime::SEED_ONLY, InformationRegime::FULL]
        );
        assert!(matches!(spec.evaluation, EvaluationContext::None));
        assert!(matches!(
            milestone_spec(ask(), &source, true)
                .expect("seeds")
                .evaluation,
            EvaluationContext::GenerationAxes { .. }
        ));
    }

    #[test]
    fn installing_picks_the_opposite_corners_as_a_and_b() {
        let source = demo();
        let spec = milestone_spec(ask(), &source, false).expect("seeds");
        let mut panel = ObservatoryPanel::default();
        panel.install(LoadedExperiment::run(&spec, &source, None).expect("runs"));
        let view = &panel.loaded.as_ref().expect("installed").view;
        assert_eq!(panel.a, view.cell_index(0, 0));
        assert_eq!(panel.b, view.cell_index(1, 1));
    }

    #[test]
    fn an_unreadable_bundle_is_refused_not_installed() {
        assert!(matches!(
            LoadedExperiment::open_json("{", Origin::File("broken.json".to_owned())),
            Err(BundleError::Malformed(_))
        ));
    }

    #[test]
    fn a_comparison_without_a_number_says_why_and_shows_none() {
        assert_eq!(
            comparison_text(Comparison::Unavailable(Unavailable::IncompatibleIdentity)),
            "not comparable: different measurement context"
        );
        assert_eq!(
            comparison_text(Comparison::Unavailable(Unavailable::NotAnEvaluation)),
            "not comparable: not an evaluation"
        );
        assert_eq!(
            comparison_text(Comparison::Unavailable(Unavailable::Missing)),
            "not measured on both"
        );
        assert_eq!(comparison_text(Comparison::Available(0.25)), "+0.250");
        assert_eq!(comparison_text(Comparison::Available(-0.125)), "-0.125");
        for unavailable in [
            Unavailable::IncompatibleIdentity,
            Unavailable::NotAnEvaluation,
            Unavailable::Missing,
        ] {
            let text = comparison_text(Comparison::Unavailable(unavailable));
            assert!(
                !text.chars().any(|c| c.is_ascii_digit()),
                "no number hides in {text:?}"
            );
        }
    }

    #[test]
    fn regimes_contributions_and_refusals_have_words() {
        assert_eq!(
            regime_text(RegimeName::SeedOnly, InformationRegime::SEED_ONLY),
            "seed only"
        );
        assert_eq!(
            regime_text(RegimeName::Full, InformationRegime::FULL),
            "full (rhythms + references + gesture)"
        );
        assert_eq!(
            regime_text(
                RegimeName::Custom,
                InformationRegime {
                    rhythms: true,
                    references: false,
                    gesture: true
                }
            ),
            "rhythms + gesture"
        );
        assert_eq!(
            contribution_text(CorpusContribution {
                templates: 0,
                references: 0,
                gesture: false
            }),
            "nothing (seed only)"
        );
        assert_eq!(
            contribution_text(CorpusContribution {
                templates: 37,
                references: 214,
                gesture: true
            }),
            "37 rhythm templates · 214 references · gesture carved"
        );
        assert_eq!(
            refusal_text(CellRefusal::EmptySet),
            "refused: empty candidate set"
        );
    }
}
