//! Generator Observatory presentation (ADR-0034): what a frontend shows of an
//! experiment.
//!
//! **One display path.** An [`ExperimentView`] is built from an
//! [`ExperimentBundleV1`] and from nothing else — not from a live
//! `ExperimentRun`, a runner, a ranked set, or a corpus. A fresh run is written
//! down as a bundle before it is shown, and a saved bundle is read back as the
//! same bundle, so both reach the screen through this one projection and cannot
//! drift into two observatories that happen to look alike.
//!
//! **No arithmetic of its own.** Every difference a frontend displays comes
//! from `griff_experiment::delta` or `griff_experiment::interaction`, verbatim.
//! When those say `Unavailable`, the view carries that reason, not a number.
//!
//! Pure, wasm-safe, typed: fingerprints stay fingerprints, refusals stay
//! refusals, and a renderer builds its own words.

use griff_core::generation_input::CorpusContribution;
use griff_core::score::Score;
use griff_experiment::{
    BundleError, CellRefusal, Comparison, Diagnostic, ExperimentBundleV1, Fingerprint,
    InformationRegime, MetricKind, MetricValue,
};

/// A named information regime, for labelling. `Custom` is any combination the
/// five presets do not name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegimeName {
    /// No channel.
    SeedOnly,
    /// Rhythm templates only.
    RhythmsOnly,
    /// References only.
    ReferencesOnly,
    /// Gesture only.
    GestureOnly,
    /// Every channel.
    Full,
    /// Another combination.
    Custom,
}

impl RegimeName {
    /// The name of `regime`.
    #[must_use]
    pub const fn of(regime: InformationRegime) -> Self {
        let _ = regime;
        Self::Custom
    }
}

/// A pipeline stage, for labelling a recorded identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StageKind {
    /// Generator.
    Generator,
    /// Scorer.
    Scorer,
    /// Selector.
    Selector,
    /// Realizer.
    Realizer,
}

/// A variant's stage identity, as the bundle recorded it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StageView {
    /// Which stage.
    pub stage: StageKind,
    /// The recorded policy id.
    pub id: String,
    /// The recorded version.
    pub version: u32,
}

/// A variant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VariantView {
    /// Its label.
    pub label: String,
    /// Its four stage identities, in pipeline order.
    pub stages: Vec<StageView>,
}

/// A requested regime.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegimeView {
    /// The channels it opens.
    pub channels: InformationRegime,
    /// Its name.
    pub name: RegimeName,
}

/// The bound population, as recorded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PopulationView {
    /// Rhythm templates.
    pub rhythm_count: usize,
    /// References.
    pub reference_count: usize,
    /// Whether it carries a gesture.
    pub gesture_present: bool,
    /// Records the loader skipped.
    pub skipped: usize,
    /// Its identity.
    pub whole: Fingerprint,
}

/// The evaluation context, as recorded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EvaluationView {
    /// No evaluator: cross-regime comparisons are unavailable.
    None,
    /// The generation-axes evaluator.
    GenerationAxes {
        /// Evaluator id.
        evaluator: String,
        /// Evaluator version.
        version: u32,
        /// Supplied references.
        references: usize,
        /// The context's identity.
        context: Fingerprint,
    },
}

/// What a cell's pass actually did — kept apart from what was requested.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EffectiveView {
    /// What the pass could consume.
    pub information: Fingerprint,
    /// What produced the cell.
    pub recipe: Fingerprint,
    /// What the corpus actually contributed.
    pub contribution: CorpusContribution,
    /// Candidates ranked.
    pub candidate_count: usize,
}

/// A cell's outcome.
#[derive(Debug, Clone, PartialEq)]
pub enum CellOutcomeView {
    /// A result: its recorded score and facts.
    Produced {
        /// The recorded score — what auditioning plays.
        score: Score,
        /// Its content identity.
        content: Fingerprint,
        /// Its metrics, each with its comparability identity.
        metrics: Vec<MetricValue>,
        /// How it was selected.
        diagnostics: Vec<Diagnostic>,
    },
    /// No result, and the typed reason. There is no score to play.
    Refused(CellRefusal),
}

/// One variant under one requested regime.
#[derive(Debug, Clone, PartialEq)]
pub struct CellView {
    /// Index into [`ExperimentView::variants`].
    pub variant: usize,
    /// Index into [`ExperimentView::regimes`].
    pub regime: usize,
    /// What was asked.
    pub requested: Fingerprint,
    /// What actually happened.
    pub effective: EffectiveView,
    /// The outcome.
    pub outcome: CellOutcomeView,
    /// The cell's record identity.
    pub record: Fingerprint,
}

/// A whole experiment, arranged for display.
#[derive(Debug, Clone, PartialEq)]
pub struct ExperimentView {
    /// The run record — the identity of everything shown.
    pub record: Fingerprint,
    /// The spec identity.
    pub spec: Fingerprint,
    /// The source identity.
    pub source: Fingerprint,
    /// The evaluation context.
    pub evaluation: EvaluationView,
    /// The bound population, if any.
    pub population: Option<PopulationView>,
    /// Variants, in spec order.
    pub variants: Vec<VariantView>,
    /// Requested regimes, in spec order.
    pub regimes: Vec<RegimeView>,
    /// Cells, variant × regime.
    pub cells: Vec<CellView>,
}

/// One metric of an A/B comparison. The values are shown as recorded; the
/// comparison is `griff_experiment::delta`'s, never a subtraction done here.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MetricComparison {
    /// Evaluation or policy objective.
    pub kind: MetricKind,
    /// The metric.
    pub name: &'static str,
    /// A's value, if measured.
    pub a: Option<f64>,
    /// B's value, if measured.
    pub b: Option<f64>,
    /// `B − A`, or why there is none.
    pub comparison: Comparison,
}

/// One evaluation metric of a 2 × 2 interaction, from
/// `griff_experiment::interaction`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct InteractionView {
    /// The metric.
    pub name: &'static str,
    /// `(B1 − A1) − (B0 − A0)`, or why there is none.
    pub comparison: Comparison,
}

impl ExperimentView {
    /// Arranges `bundle` for display. Reads the bundle only.
    ///
    /// # Errors
    /// Whatever rebuilding the bundle's typed run refuses (a projection the
    /// model cannot hold, a name outside the vocabulary).
    pub fn from_bundle(bundle: &ExperimentBundleV1) -> Result<Self, BundleError> {
        let _ = bundle;
        Err(BundleError::NotThisRun)
    }

    /// The cell of `variant` under regime `regime` (indices into the view).
    #[must_use]
    pub fn cell_index(&self, variant: usize, regime: usize) -> Option<usize> {
        let _ = (variant, regime);
        None
    }

    /// Cell `a` against cell `b`, metric by metric, in A's order then B's
    /// extras.
    #[must_use]
    pub fn compare(&self, a: usize, b: usize) -> Vec<MetricComparison> {
        let _ = (a, b);
        Vec::new()
    }

    /// The interaction of variants `(va, vb)` across regimes `(r0, r1)`, for
    /// every evaluation metric any of the four cells measured.
    #[must_use]
    pub fn interaction(
        &self,
        variants: (usize, usize),
        regimes: (usize, usize),
    ) -> Vec<InteractionView> {
        let _ = (variants, regimes);
        Vec::new()
    }
}

#[cfg(test)]
mod tests;
