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

use griff_core::generation_input::{CorpusContribution, CorpusMaterial, GenerationAsk};
use griff_core::score::Score;
use griff_experiment::{
    BundleError, CellRefusal, Comparison, ExperimentBundleV1, ExperimentSpec, InformationRegime,
    RunError,
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
        let _ = (bundle, origin);
        Err(BundleError::NotThisRun)
    }

    /// Reads, verifies, and arranges a saved bundle.
    ///
    /// # Errors
    /// Whatever loading or arranging refuses.
    pub fn open_json(json: &str, origin: Origin) -> Result<Self, BundleError> {
        let _ = (json, origin);
        Err(BundleError::NotThisRun)
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
        let _ = (spec, source, corpus);
        Err(RunFailure::Bundle(BundleError::NotThisRun))
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
    let _ = (ask, source, evaluate_against_source);
    Err(String::new())
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
        let _ = loaded;
    }
}

/// A comparison, in words: the signed number when the experiment API gives
/// one, and otherwise why there is none.
#[must_use]
pub fn comparison_text(comparison: Comparison) -> String {
    let _ = comparison;
    String::new()
}

/// A requested regime, in words.
#[must_use]
pub fn regime_text(name: RegimeName, channels: InformationRegime) -> String {
    let _ = (name, channels);
    String::new()
}

/// What a corpus actually contributed, in words.
#[must_use]
pub fn contribution_text(contribution: CorpusContribution) -> String {
    let _ = contribution;
    String::new()
}

/// A refusal, in words.
#[must_use]
pub fn refusal_text(refusal: CellRefusal) -> String {
    let _ = refusal;
    String::new()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::panic)]

    use super::*;
    use griff_core::import::import_score_auto;
    use griff_experiment::{CellOutcomeV1, CellRefusalV1, Unavailable};
    use griff_ui_core::observatory::CellOutcomeView;

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
        assert!(matches!(
            spec.evaluation,
            griff_experiment::EvaluationContext::None
        ));
        assert!(matches!(
            milestone_spec(ask(), &source, true)
                .expect("seeds")
                .evaluation,
            griff_experiment::EvaluationContext::GenerationAxes { .. }
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
    fn a_refused_cell_arranges_as_its_refusal() {
        let source = demo();
        let spec = milestone_spec(ask(), &source, false).expect("seeds");
        let mut bundle = LoadedExperiment::run(&spec, &source, None)
            .expect("runs")
            .bundle;
        bundle.cells[3].outcome = CellOutcomeV1::Refused(CellRefusalV1::EmptySet);
        let loaded = LoadedExperiment::from_bundle(bundle, Origin::Run).expect("arranges");
        assert_eq!(
            loaded.view.cells[3].outcome,
            CellOutcomeView::Refused(CellRefusal::EmptySet)
        );
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
