//! Exact analysis of a fingering objective's **optimum set**, and a learned
//! **secondary objective** that breaks its ties.
//!
//! The optimality-gap audit (`docs/audit/2026-09-fingering-optimality-gap.md`)
//! found the production DP exact but its fitted weights under-discriminative:
//! many fingerings tie at the optimum, and the DP's fixed tie-break keeps far
//! less agreement with tab authors than the optimum set contains. This module
//! measures that set exactly — how many optimal paths, and the least, most and
//! expected agreement with a reference among them — with chain DPs instead of
//! an external solver, and learns a human-blind tie-break over it: primary cost
//! first, a learned secondary cost second, both minimized lexicographically.
//!
//! Research tooling only (lab crate); nothing here is a production dependency.

use griff_core::event::{FretboardPosition, Pitch, Tuning};
use griff_core::fretboard::FingeringWeights;

use crate::problems::LabError;

/// Number of secondary features ([`FEATURE_NAMES`]).
pub const FEATURES: usize = 20;

/// Secondary feature names, in [`Features`] order. Per note: `fret`, `open`,
/// one-hot `string_1` … `string_7` (strings above 7 count as 7). Per
/// transition (Δ = this note − previous note): `fret_distance` |Δfret|,
/// `string_distance` |Δstring|, `string_change` [Δstring ≠ 0], `same_fret`
/// [Δfret = 0, both fretted], `span_over_3` / `span_over_5` [|Δfret| > 3 / 5,
/// both fretted], `open_transition` [either open], `diagonal` [Δstring ≠ 0 and
/// Δfret ≠ 0], `toward_high_string` [Δstring < 0], `fret_up` [Δfret > 0],
/// `box_move` [Δstring and Δfret nonzero with the same sign].
pub const FEATURE_NAMES: [&str; FEATURES] = [
    "fret",
    "open",
    "string_1",
    "string_2",
    "string_3",
    "string_4",
    "string_5",
    "string_6",
    "string_7",
    "fret_distance",
    "string_distance",
    "string_change",
    "same_fret",
    "span_over_3",
    "span_over_5",
    "open_transition",
    "diagonal",
    "toward_high_string",
    "fret_up",
    "box_move",
];

/// A feature vector (or a weight vector over it).
pub type Features = [i64; FEATURES];

/// A chain-structured fingering objective over one line: per note the
/// candidate positions (in [`Tuning::candidates`] order) with unary costs, and
/// a cost for every transition between consecutive candidates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Chain {
    positions: Vec<Vec<FretboardPosition>>,
    unary: Vec<Vec<i64>>,
    /// `pairwise[i][a][b]`: candidate `a` of note `i − 1` to candidate `b` of
    /// note `i`; `pairwise[0]` is empty.
    pairwise: Vec<Vec<Vec<i64>>>,
}

impl Chain {
    /// The production `v1` objective (as `griff_core::fretboard::infer_positions`
    /// minimizes it) as a chain.
    ///
    /// # Errors
    ///
    /// [`LabError::EmptyLine`] for no pitches; [`LabError::UnpositionablePitch`]
    /// when a pitch has no candidate at or below `max_fret`.
    pub fn v1(
        pitches: &[Pitch],
        tuning: &Tuning,
        weights: &FingeringWeights,
        max_fret: u8,
    ) -> Result<Self, LabError> {
        let _ = (pitches, tuning, weights, max_fret);
        todo!("v1 chain — green step")
    }

    /// Notes in the line.
    #[must_use]
    pub fn len(&self) -> usize {
        self.positions.len()
    }

    /// `true` when the line has no notes (not constructible via [`Chain::v1`]).
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.positions.is_empty()
    }

    /// Candidate positions of note `note` (empty when out of range).
    #[must_use]
    pub fn candidates(&self, note: usize) -> &[FretboardPosition] {
        self.positions.get(note).map_or(&[], Vec::as_slice)
    }

    /// Primary cost of a path given as one candidate index per note; `None`
    /// for a ragged path or an out-of-range index.
    #[must_use]
    pub fn cost(&self, path: &[usize]) -> Option<i64> {
        let _ = path;
        todo!("chain cost — green step")
    }

    /// The positions a path selects; `None` as for [`Chain::cost`].
    #[must_use]
    pub fn positions_of(&self, path: &[usize]) -> Option<Vec<FretboardPosition>> {
        let _ = path;
        todo!("path positions — green step")
    }
}

/// A path count: exact while it fits `u64`, with its natural logarithm always.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PathCount {
    /// The count, saturated at `u64::MAX`.
    pub exact: u64,
    /// `true` when the true count exceeds `u64::MAX`.
    pub saturated: bool,
    /// Natural logarithm of the true count.
    pub ln: f64,
}

/// Agreement with a reference over the optimum set.
#[derive(Debug, Clone, PartialEq)]
pub struct AgreementRange {
    /// Fewest reference matches of any optimal path.
    pub min: usize,
    /// Most reference matches of any optimal path — the ceiling any
    /// tie-break can reach.
    pub max: usize,
    /// Expected matches when an optimal path is drawn uniformly at random.
    pub expected: f64,
    /// An optimal path attaining `max` (ties: lowest candidate indices, as
    /// the production DP breaks them) — the achievable target for learning.
    pub best_path: Vec<usize>,
}

/// The optimum set of a chain.
#[derive(Debug, Clone, PartialEq)]
pub struct OptimumSet {
    /// The optimal primary cost.
    pub optimum: i64,
    /// How many paths attain it.
    pub count: PathCount,
    /// Agreement with the reference, when one of the chain's length was given.
    pub agreement: Option<AgreementRange>,
}

/// Exactly analyses the optimum set of `chain` (forward/backward counting and
/// lexicographic DPs; no search). `reference` positions are compared per note;
/// a reference of a different length yields `agreement: None`.
#[must_use]
pub fn optimum_set(chain: &Chain, reference: Option<&[FretboardPosition]>) -> OptimumSet {
    let _ = (chain, reference);
    todo!("optimum set — green step")
}

/// Reference matches of a path; `None` for a ragged path or reference.
#[must_use]
pub fn path_matches(
    chain: &Chain,
    path: &[usize],
    reference: &[FretboardPosition],
) -> Option<usize> {
    let _ = (chain, path, reference);
    todo!("path matches — green step")
}

/// Secondary features of a path, summed over notes and transitions
/// ([`FEATURE_NAMES`]); `None` as for [`Chain::cost`].
#[must_use]
pub fn path_features(chain: &Chain, path: &[usize]) -> Option<Features> {
    let _ = (chain, path);
    todo!("path features — green step")
}

/// The path minimizing `(primary cost, secondary cost)` lexicographically,
/// secondary cost = `weights · features` (+ `margin` per note that matches the
/// reference when `augment = Some((reference, margin))` — loss-augmented
/// inference: it prefers cheap paths that *disagree*). Remaining ties keep the
/// lowest candidate indices, so zero weights and no augmentation reproduce the
/// production DP's path exactly.
#[must_use]
pub fn lexicographic_path(
    chain: &Chain,
    weights: &Features,
    augment: Option<(&[FretboardPosition], i64)>,
) -> Vec<usize> {
    let _ = (chain, weights, augment);
    todo!("lexicographic path — green step")
}

/// One training line: its primary chain and the tab author's positions.
#[derive(Debug, Clone)]
pub struct Example {
    /// The primary objective over the line.
    pub chain: Chain,
    /// The tab author's positions, one per note.
    pub human: Vec<FretboardPosition>,
}

/// Perceptron settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PerceptronConfig {
    /// Maximum passes over the examples.
    pub epochs: usize,
    /// Loss augmentation per agreeing note during training (0 = plain
    /// perceptron).
    pub margin: i64,
}

/// A trained secondary objective.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrainedSecondary {
    /// Averaged weights (the running sum of per-example weights — the same
    /// argmin as the average, kept in integers).
    pub weights: Features,
    /// Updates made.
    pub updates: u64,
    /// Epochs run (fewer than configured when an epoch made no update).
    pub epochs: usize,
}

/// Learns secondary weights with an averaged, loss-augmented structured
/// perceptron **inside the primary optimum set**. The target per example is
/// the achievable one — [`AgreementRange::best_path`], not the human path,
/// which is often not primary-optimal. When the (augmented) prediction agrees
/// with the tab author less than the target does, the weights move by
/// `features(prediction) − features(target)`. Deterministic: examples in the
/// given order, integer arithmetic.
#[must_use]
pub fn train_secondary(examples: &[Example], config: &PerceptronConfig) -> TrainedSecondary {
    let _ = (examples, config);
    todo!("secondary perceptron — green step")
}
