//! Generator Observatory presentation (ADR-0034): what a frontend shows of an
//! experiment.
//!
//! **One display path.** An [`ExperimentView`] is built from an
//! [`ExperimentBundleV1`] that verifies, and from nothing else — not from a live
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
    delta, interaction, BundleError, CellOutcome, CellRefusal, Comparison, Diagnostic,
    EvaluationContextV1, ExperimentBundleV1, ExperimentRun, Fingerprint, InformationRegime,
    MetricKind, MetricValue, Mismatch, PolicyIdentityV1,
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
        match (regime.rhythms, regime.references, regime.gesture) {
            (false, false, false) => Self::SeedOnly,
            (true, false, false) => Self::RhythmsOnly,
            (false, true, false) => Self::ReferencesOnly,
            (false, false, true) => Self::GestureOnly,
            (true, true, true) => Self::Full,
            _ => Self::Custom,
        }
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
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    /// Verifies `bundle`, then arranges it for display. Reads the bundle only.
    ///
    /// The bundle is verified here, whichever way it arrived: `from_json` and
    /// `from_run` verify too, but an [`ExperimentBundleV1`] is a plain value
    /// that can be edited after either, and this is the frontend boundary.
    ///
    /// # Errors
    /// [`BundleError::IdentityMismatch`] for data its recorded identities do
    /// not describe; whatever rebuilding the typed run refuses (a projection
    /// the model cannot hold, a name outside the vocabulary).
    pub fn from_bundle(bundle: &ExperimentBundleV1) -> Result<Self, BundleError> {
        bundle.verify()?;
        let run = bundle.run()?;
        let regimes: Vec<RegimeView> = bundle
            .spec
            .regimes
            .iter()
            .map(|&recorded| {
                let channels = InformationRegime::from(recorded);
                RegimeView {
                    channels,
                    name: RegimeName::of(channels),
                }
            })
            .collect();
        let variants = bundle
            .spec
            .variants
            .iter()
            .map(|variant| VariantView {
                label: variant.label.clone(),
                stages: vec![
                    stage(StageKind::Generator, &variant.generator.identity),
                    stage(StageKind::Scorer, &variant.scorer.identity),
                    stage(StageKind::Selector, &variant.selector.identity),
                    stage(StageKind::Realizer, &variant.realizer.identity),
                ],
            })
            .collect();
        let evaluation = evaluation_view(&bundle.spec.evaluation, run.evaluation);
        let population = run.corpus.as_ref().map(|snapshot| PopulationView {
            rhythm_count: snapshot.rhythm_count,
            reference_count: snapshot.reference_count,
            gesture_present: snapshot.gesture_present,
            skipped: snapshot.skipped.len(),
            whole: snapshot.whole,
        });
        let cells = cell_views(&run, &regimes)?;
        Ok(Self {
            record: run.record,
            spec: run.spec,
            source: run.source,
            evaluation,
            population,
            variants,
            regimes,
            cells,
        })
    }

    /// The cell of `variant` under regime `regime` (indices into the view).
    #[must_use]
    pub fn cell_index(&self, variant: usize, regime: usize) -> Option<usize> {
        self.cells
            .iter()
            .position(|cell| cell.variant == variant && cell.regime == regime)
    }

    /// The metrics of cell `i`; none for a refused or unknown cell.
    fn metrics(&self, i: usize) -> &[MetricValue] {
        match self.cells.get(i).map(|cell| &cell.outcome) {
            Some(CellOutcomeView::Produced { metrics, .. }) => metrics,
            _ => &[],
        }
    }

    /// Cell `a` against cell `b`, metric by metric, in the order A measured
    /// them and then whatever only B measured.
    #[must_use]
    pub fn compare(&self, a: usize, b: usize) -> Vec<MetricComparison> {
        let (from, to) = (self.metrics(a), self.metrics(b));
        let mut keys: Vec<(MetricKind, &'static str)> = Vec::new();
        for metric in from.iter().chain(to) {
            let key = (metric.identity.kind, metric.identity.name);
            if !keys.contains(&key) {
                keys.push(key);
            }
        }
        keys.into_iter()
            .map(|(kind, name)| {
                let (ma, mb) = (find(from, kind, name), find(to, kind, name));
                MetricComparison {
                    kind,
                    name,
                    a: ma.map(|m| m.value),
                    b: mb.map(|m| m.value),
                    comparison: delta(ma, mb),
                }
            })
            .collect()
    }

    /// The interaction of variants `(va, vb)` across regimes `(r0, r1)`, for
    /// every evaluation metric any of the four cells measured.
    #[must_use]
    pub fn interaction(
        &self,
        variants: (usize, usize),
        regimes: (usize, usize),
    ) -> Vec<InteractionView> {
        let ((va, vb), (r0, r1)) = (variants, regimes);
        let corner = |v, r| {
            self.cell_index(v, r)
                .map_or(NO_METRICS, |i| self.metrics(i))
        };
        let corners = [
            corner(va, r0),
            corner(vb, r0),
            corner(va, r1),
            corner(vb, r1),
        ];
        let mut names: Vec<&'static str> = Vec::new();
        for metric in corners.iter().flat_map(|metrics| metrics.iter()) {
            if metric.identity.kind == MetricKind::Evaluation
                && !names.contains(&metric.identity.name)
            {
                names.push(metric.identity.name);
            }
        }
        names
            .into_iter()
            .map(|name| {
                let [a0, b0, a1, b1] =
                    corners.map(|metrics| find(metrics, MetricKind::Evaluation, name));
                InteractionView {
                    name,
                    comparison: interaction(a0, b0, a1, b1),
                }
            })
            .collect()
    }
}

/// The recorded evaluation context, arranged.
fn evaluation_view(recorded: &EvaluationContextV1, context: Option<Fingerprint>) -> EvaluationView {
    match (recorded, context) {
        (
            EvaluationContextV1::GenerationAxes {
                evaluator,
                references,
                ..
            },
            Some(context),
        ) => EvaluationView::GenerationAxes {
            evaluator: evaluator.id.clone(),
            version: evaluator.version,
            references: references.len(),
            context,
        },
        _ => EvaluationView::None,
    }
}

/// Every cell of `run`, placed on the view's regimes.
fn cell_views(run: &ExperimentRun, regimes: &[RegimeView]) -> Result<Vec<CellView>, BundleError> {
    run.cells
        .iter()
        .enumerate()
        .map(|(i, cell)| {
            let unplaced = || BundleError::IdentityMismatch(Mismatch::CellPass { cell: i });
            let regime = regimes
                .iter()
                .position(|r| r.channels == cell.regime)
                .ok_or_else(unplaced)?;
            let pass = run.passes.get(cell.pass).ok_or_else(unplaced)?;
            Ok(CellView {
                variant: cell.variant,
                regime,
                requested: cell.requested,
                effective: EffectiveView {
                    information: pass.information,
                    recipe: cell.recipe,
                    contribution: pass.contribution,
                    candidate_count: pass.candidate_count,
                },
                outcome: match &cell.outcome {
                    CellOutcome::Produced(result) => CellOutcomeView::Produced {
                        score: result.score.clone(),
                        content: result.content,
                        metrics: result.metrics.clone(),
                        diagnostics: result.diagnostics.clone(),
                    },
                    CellOutcome::Refused(refusal) => CellOutcomeView::Refused(*refusal),
                },
                record: cell.record,
            })
        })
        .collect()
}

/// A cell with nothing measured.
const NO_METRICS: &[MetricValue] = &[];

/// A recorded stage identity.
fn stage(stage: StageKind, identity: &PolicyIdentityV1) -> StageView {
    StageView {
        stage,
        id: identity.id.clone(),
        version: identity.version,
    }
}

/// The metric of `kind` named `name`, if measured.
fn find<'a>(metrics: &'a [MetricValue], kind: MetricKind, name: &str) -> Option<&'a MetricValue> {
    metrics
        .iter()
        .find(|m| m.identity.kind == kind && m.identity.name == name)
}

#[cfg(test)]
mod tests;
