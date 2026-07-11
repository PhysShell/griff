//! Candidate set + explainable rerank over the S6 strategies (ADR-0017;
//! melodic-closure note §7.2/§7.3).
//!
//! S6 [`generate`](crate::generate::generate) produces exactly one candidate
//! from one strategy; the S6 stage doc promises a *set*, and the ADR-0017
//! scoring vocabulary exists precisely to rank one. This module is that seam,
//! as two pure functions:
//!
//! - [`generate_candidate_set`] fans a base request out over **every** S6
//!   strategy × `variants_per_strategy` seed variants. Variant seeds are
//!   derived from the base seed with a `SplitMix64` mix over
//!   `(strategy index, variant index)` — deterministic (SPEC §6) and
//!   independent of whether gesture carving is on, so gestured and plain runs
//!   of the same request pair up seed-for-seed. `RhythmCopyPitchSubstitute`
//!   is skipped (not an error) when no usable rhythm template exists; when
//!   several templates exist, variants rotate through them so a
//!   multi-template corpus is audible in the set. With a
//!   [`GestureControl`] the candidates are carved through the S6 gesture
//!   compiler ([`generate_gestured`], research note §3.5) instead of staying
//!   wall-to-wall.
//! - [`rerank_candidates`] scores each candidate on the four closure axes
//!   ([`closure_axes`]) plus the two novelty axes ([`novelty_axes`] against
//!   caller-supplied reference scores) under a caller-supplied
//!   [`WeightPolicy`], and returns [`Scored`] envelopes in rank order
//!   (aggregate descending, ties by candidate index — [`rank_indices`]).
//!   Weights are data (ADR-0017 §3): [`rerank_weights_v1`] is the untuned
//!   uniform baseline, and rejection thresholds (e.g. a novelty cut) remain
//!   the caller's policy.

use crate::closure::{closure_axes, ClosureError};
use crate::event::Ticks;
use crate::generate::{
    generate, GenerationConstraints, GenerationError, GenerationSeed, GenerationStrategy,
    PitchMaterial, RuleGenerationRequest,
};
use crate::gesture::{generate_gestured, GestureControl, GestureGenError};
use crate::novelty::{measure_novelty, novelty_axes, NoveltyError};
use crate::score::Score;
use crate::scoring::{rank_indices, Axes, Scored, WeightPolicy};

/// The rerank axes, in canonical order: the four closure axes followed by the
/// two novelty axes (ADR-0017).
pub const RERANK_AXIS_LABELS: [&str; 6] = [
    "internal_continuity",
    "ending_stability",
    "final_lengthening",
    "gap_fill",
    "quote_novelty",
    "ngram_novelty",
];

/// The baseline rerank weight policy (`generation_rerank` v1): uniform over
/// the six axes.
///
/// Untuned by design — weights are data the feedback layer (S9) learns
/// (ADR-0017 §3), and the swancore-specific weighting awaits corpus
/// calibration.
#[must_use]
pub fn rerank_weights_v1() -> WeightPolicy {
    WeightPolicy::uniform("generation_rerank", 1, &RERANK_AXIS_LABELS)
}

/// Every S6 strategy, in declaration order — the fan-out axis of the set.
const SET_STRATEGIES: [GenerationStrategy; 5] = [
    GenerationStrategy::RhythmCopyPitchSubstitute,
    GenerationStrategy::MotifTransposeVariation,
    GenerationStrategy::ConstrainedRandomWalk,
    GenerationStrategy::ShuffleMotifs,
    GenerationStrategy::RepeatVariation,
];

/// Everything a candidate-set pass needs: the S6 request fields minus the
/// single strategy, plus the fan-out width and the optional gesture ask.
#[derive(Debug, Clone)]
pub struct SetRequest {
    /// Base deterministic seed; variant seeds are derived from it.
    pub seed: GenerationSeed,
    /// Scale to draw pitches from.
    pub pitch_material: PitchMaterial,
    /// Structural constraints applied to every candidate.
    pub constraints: GenerationConstraints,
    /// Rhythm templates (inner `Vec` = note durations per bar), typically
    /// extracted from corpus chunks. Empty templates are ignored; with none
    /// usable, `RhythmCopyPitchSubstitute` is skipped.
    pub source_rhythms: Vec<Vec<Ticks>>,
    /// Seed variants generated per strategy (must be ≥ 1).
    pub variants_per_strategy: usize,
    /// When set, every candidate is carved through the gesture compiler.
    pub gesture: Option<GestureControl>,
}

/// One candidate of the set, with full provenance.
#[derive(Debug, Clone)]
pub struct SetCandidate {
    /// The generated (and possibly gesture-carved) score.
    pub score: Score,
    /// Strategy that produced this candidate.
    pub strategy: GenerationStrategy,
    /// Derived variant seed this candidate ran under.
    pub seed: GenerationSeed,
    /// The gesture ask the candidate was carved against, when one was.
    pub gesture: Option<GestureControl>,
}

/// Errors the candidate-set builder can emit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetError {
    /// `variants_per_strategy` is zero.
    VariantCountZero,
    /// The underlying S6 generator rejected a derived request.
    Generation(GenerationError),
    /// The gesture compiler rejected the control or a derived request.
    Gesture(GestureGenError),
}

impl From<GenerationError> for SetError {
    fn from(e: GenerationError) -> Self {
        Self::Generation(e)
    }
}

impl From<GestureGenError> for SetError {
    fn from(e: GestureGenError) -> Self {
        Self::Gesture(e)
    }
}

/// Generates the full candidate set for `request`.
///
/// Fully deterministic: the same request always produces the same set, and
/// variant seeds do not depend on the gesture ask (so gestured and plain runs
/// pair up seed-for-seed).
pub fn generate_candidate_set(request: &SetRequest) -> Result<Vec<SetCandidate>, SetError> {
    if request.variants_per_strategy == 0 {
        return Err(SetError::VariantCountZero);
    }

    let templates: Vec<&Vec<Ticks>> = request
        .source_rhythms
        .iter()
        .filter(|t| !t.is_empty())
        .collect();

    let mut set = Vec::new();
    for (si, strategy) in SET_STRATEGIES.iter().enumerate() {
        let needs_template = *strategy == GenerationStrategy::RhythmCopyPitchSubstitute;
        if needs_template && templates.is_empty() {
            continue;
        }
        for variant in 0..request.variants_per_strategy {
            let seed = GenerationSeed(derive_seed(request.seed.0, si, variant));
            // Rotate templates across variants so each template gets heard;
            // strategies other than rhythm-copy ignore the field entirely.
            let source_rhythms = if needs_template {
                let idx = variant.checked_rem(templates.len()).unwrap_or(0);
                vec![templates.get(idx).map_or_else(Vec::new, |t| (*t).clone())]
            } else {
                Vec::new()
            };
            let sub = RuleGenerationRequest {
                seed,
                pitch_material: request.pitch_material.clone(),
                constraints: request.constraints,
                source_rhythms,
                strategy: *strategy,
            };
            let (score, gesture) = match request.gesture {
                Some(control) => (generate_gestured(&sub, control)?.score, Some(control)),
                None => (generate(&sub)?.score, None),
            };
            set.push(SetCandidate {
                score,
                strategy: *strategy,
                seed,
                gesture,
            });
        }
    }
    Ok(set)
}

/// Scores `candidates` on the six rerank axes and returns them as [`Scored`]
/// envelopes in rank order (aggregate descending, ties by candidate index).
///
/// Novelty is measured against `references` (corpus chunk scores); an empty
/// reference set reads as fully novel. A candidate whose first track cannot
/// be measured is dropped from the ranking — unreachable for S6/gestured
/// output, which always keeps at least one note, but stated rather than
/// silently mis-scored.
#[must_use]
pub fn rerank_candidates(
    candidates: Vec<SetCandidate>,
    material: &PitchMaterial,
    references: &[Score],
    policy: &WeightPolicy,
) -> Vec<Scored<SetCandidate>> {
    let scored: Vec<Scored<SetCandidate>> = candidates
        .into_iter()
        .filter_map(|candidate| {
            let axes = candidate_axes(&candidate.score, material, references).ok()?;
            let seed = candidate.seed.0;
            Some(Scored::new(candidate, axes, policy, Some(seed)))
        })
        .collect();

    let order = rank_indices(&scored);
    let mut slots: Vec<Option<Scored<SetCandidate>>> = scored.into_iter().map(Some).collect();
    order
        .into_iter()
        .filter_map(|i| slots.get_mut(i).and_then(Option::take))
        .collect()
}

/// The six rerank axes of a candidate score's first track: closure then
/// novelty, in [`RERANK_AXIS_LABELS`] order.
fn candidate_axes(
    score: &Score,
    material: &PitchMaterial,
    references: &[Score],
) -> Result<Axes, MeasureError> {
    let closure = closure_axes(score, 0, material)?;
    let novelty = novelty_axes(&measure_novelty(score, 0, references)?);
    Ok(Axes::new(
        closure.iter().chain(novelty.iter()).copied().collect(),
    ))
}

/// Internal measurement failure (candidate dropped from the ranking).
enum MeasureError {
    Closure,
    Novelty,
}

impl From<ClosureError> for MeasureError {
    fn from(_: ClosureError) -> Self {
        Self::Closure
    }
}

impl From<NoveltyError> for MeasureError {
    fn from(_: NoveltyError) -> Self {
        Self::Novelty
    }
}

/// `SplitMix64` finalizer — mixes `(base, strategy, variant)` into a variant
/// seed with good avalanche, so neighbouring variants do not correlate.
fn derive_seed(base: u64, strategy_index: usize, variant: usize) -> u64 {
    let si = u64::try_from(strategy_index).unwrap_or(u64::MAX);
    let vi = u64::try_from(variant).unwrap_or(u64::MAX);
    let mut z = base
        .wrapping_add(si.wrapping_mul(0x9E37_79B9_7F4A_7C15))
        .wrapping_add(vi.wrapping_mul(0xBF58_476D_1CE4_E5B9));
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}
