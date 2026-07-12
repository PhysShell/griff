//! Shared pure-core tonal *evidence* and *inference* — the tonal-context layer
//! (Phase 1).
//!
//! Two layers, deliberately separated so measurement is a pure fact and
//! inference is a scored, *uncertain* verdict (mirroring the axes-vs-aggregate
//! split of ADR-0017):
//!
//! - [`PitchEvidence`] — the raw, observed pitch-class facts of an explicit
//!   [`EvidenceScope`] (whole score / track / voice): per-class onset counts,
//!   per-class duration mass in ticks, and the observed
//!   [`PitchRange`](crate::feature::PitchRange). No thresholds, no key; a pure
//!   projection of a [`Score`] region that is *additive* across scopes (a whole
//!   score's evidence equals the sum of its tracks', a track's the sum of its
//!   voices').
//! - [`TonalEstimate`] — a ranked 24-key Krumhansl–Schmuckler inference with an
//!   explicit [`confidence_margin`](TonalEstimate::confidence_margin); every
//!   [`TonalCandidate`] carries its tonic, [`KeyMode`], Pearson correlation and
//!   `scale_fit`.
//!
//! This generalises the previously private, single-winner
//! `complement::estimate_harmony`, which now projects the winning
//! [`TonalCandidate`] into a `HarmonicContext` — one estimator, not two
//! (heuristics-first, ADR-0008; the profiles are the same Krumhansl–Kessler
//! ratings, never ML).
//!
//! **KS v1 is duration-only.** The histogram is weighted by duration mass; raw
//! onset counts are the fallback *only* when the total duration mass is zero (a
//! part whose notes all have zero duration still estimates). Phase 1 blends
//! nothing and applies no metric-accent policy — those remain uncalibrated
//! design space (see `docs/audit/2026-07-tonal-context-phase0.md`).

use std::cmp::Ordering;

use crate::event::Pitch;
use crate::feature::PitchRange;
use crate::score::{AtomEvent, Score, Voice};

/// Krumhansl–Kessler major tonal-hierarchy profile (probe-tone ratings).
const KK_MAJOR: [f64; 12] = [
    6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88,
];
/// Krumhansl–Kessler natural-minor tonal-hierarchy profile.
const KK_MINOR: [f64; 12] = [
    6.33, 2.68, 3.52, 5.38, 2.60, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17,
];

/// Major or natural minor — the two scale shapes the key estimate considers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyMode {
    /// The major (Ionian) scale.
    Major,
    /// The natural minor (Aeolian) scale.
    Minor,
}

impl KeyMode {
    /// Semitone offsets of this mode's scale above its tonic.
    #[must_use]
    pub const fn scale_offsets(self) -> [u8; 7] {
        match self {
            Self::Major => [0, 2, 4, 5, 7, 9, 11],
            Self::Minor => [0, 2, 3, 5, 7, 8, 10],
        }
    }

    /// The Krumhansl–Kessler profile for this mode.
    const fn profile(self) -> &'static [f64; 12] {
        match self {
            Self::Major => &KK_MAJOR,
            Self::Minor => &KK_MINOR,
        }
    }
}

/// The region of a [`Score`] a [`PitchEvidence`] projection covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EvidenceScope {
    /// Every note of every voice on every track.
    WholeScore,
    /// Every voice of the track at this index.
    Track(usize),
    /// A single voice, addressed by its *position* in the track's voice list
    /// (not its [`Voice::id`](crate::score::Voice::id)).
    Voice {
        /// Track index.
        track: usize,
        /// Voice position within the track.
        voice: usize,
    },
}

/// Raw, observed pitch-class facts for an [`EvidenceScope`] — a pure projection
/// of a [`Score`] region with no thresholds and no key (glossary §8).
///
/// The two histograms are kept apart because onset salience and sustained
/// duration disagree (a pedal tone dominates `duration_mass` but not
/// `onset_counts`); inference weights them, evidence does not. Both are *raw*:
/// `onset_counts` are literal onset tallies and `duration_mass` is summed
/// sounded ticks — neither is a probability, wall-clock time, or verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PitchEvidence {
    /// The region these facts were measured over.
    pub scope: EvidenceScope,
    /// Total sounding notes in scope.
    pub note_count: usize,
    /// Per-pitch-class count of note onsets (index `0` = C … `11` = B).
    pub onset_counts: [u32; 12],
    /// Per-pitch-class summed note duration in ticks (duration *mass*, not
    /// wall-clock sounding time).
    pub duration_mass: [u64; 12],
    /// The inclusive pitch span observed, or `None` when the scope is silent.
    pub pitch_range: Option<PitchRange>,
}

impl PitchEvidence {
    /// Measures the raw pitch-class evidence of `scope` on `score`.
    ///
    /// Pure and deterministic; a silent or out-of-range scope yields zeroed
    /// histograms, `note_count` 0 and `pitch_range` `None`.
    // `score` (the Score) and `scope` (the region) are distinct domain terms.
    #[allow(clippy::similar_names)]
    #[must_use]
    pub fn measure(score: &Score, scope: EvidenceScope) -> Self {
        let mut tally = Tally::default();

        match scope {
            EvidenceScope::WholeScore => {
                for track in &score.tracks {
                    for voice in &track.voices {
                        tally.visit_voice(voice);
                    }
                }
            }
            EvidenceScope::Track(index) => {
                if let Some(track) = score.tracks.get(index) {
                    for voice in &track.voices {
                        tally.visit_voice(voice);
                    }
                }
            }
            EvidenceScope::Voice { track, voice } => {
                if let Some(v) = score.tracks.get(track).and_then(|t| t.voices.get(voice)) {
                    tally.visit_voice(v);
                }
            }
        }

        Self {
            scope,
            note_count: tally.note_count,
            onset_counts: tally.onset_counts,
            duration_mass: tally.duration_mass,
            pitch_range: tally.pitch_range,
        }
    }
}

/// One key's fit against the evidence: a tonic, a mode, the Pearson correlation
/// with that key's rotated profile, and the fraction of weight on its scale.
///
/// `scale_fit` is a *fact*, not a verdict — what counts as "fitting well enough"
/// is corpus/S9 calibration territory. It is duration-weighted whenever duration
/// mass is present and onset-count-weighted only in the zero-duration fallback,
/// exactly matching the correlation's weighting.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TonalCandidate {
    /// Tonic pitch class: `0` = C … `11` = B.
    pub tonic: u8,
    /// Major or natural minor.
    pub mode: KeyMode,
    /// Pearson correlation with the mode's profile rotated onto `tonic`.
    pub correlation: f64,
    /// Weighted fraction of the histogram on this key's scale, in `[0, 1]`.
    pub scale_fit: f64,
}

/// A ranked key estimate carrying *explicit uncertainty*.
///
/// `candidates` holds all 24 keys best-first; `confidence_margin` is the
/// correlation gap between the winner and the best rival, so a caller can tell a
/// confident estimate (wide margin) from an ambiguous one (near tie) without
/// re-running the maths. A margin near zero is *honest ambiguity*, not a defect:
/// an exactly flat histogram scores every key at a finite zero and leaves a zero
/// margin, and the deterministic C-major-first tie order that results is an
/// ordering convention, never a confidence claim.
#[derive(Debug, Clone, PartialEq)]
pub struct TonalEstimate {
    /// All 24 keys (12 tonics × 2 modes), ranked best-first.
    pub candidates: Vec<TonalCandidate>,
    /// `winner.correlation - runner_up.correlation` (`0.0` when tied).
    pub confidence_margin: f64,
}

impl TonalEstimate {
    /// The best-ranked candidate, or `None` when there are no candidates.
    #[must_use]
    pub fn winner(&self) -> Option<&TonalCandidate> {
        self.candidates.first()
    }
}

/// Infers a ranked [`TonalEstimate`] from measured [`PitchEvidence`].
///
/// Returns `None` for a silent scope (`note_count == 0`); otherwise ranks all 24
/// keys (KS v1: duration mass weights the histogram, raw onset counts are the
/// fallback only when the total duration mass is zero). Deterministic.
#[must_use]
pub fn estimate_key(evidence: &PitchEvidence) -> Option<TonalEstimate> {
    estimate_from_histograms(
        evidence.note_count,
        &evidence.onset_counts,
        &evidence.duration_mass,
    )
}

/// The shared inference core over raw histograms, reused by [`estimate_key`] and
/// by `complement::estimate_harmony` (which has weighted notes, not a scope).
#[allow(clippy::arithmetic_side_effects, clippy::indexing_slicing)]
pub(crate) fn estimate_from_histograms(
    note_count: usize,
    onset_counts: &[u32; 12],
    duration_mass: &[u64; 12],
) -> Option<TonalEstimate> {
    if note_count == 0 {
        return None;
    }

    let weights = resolve_weights(onset_counts, duration_mass);
    let candidates = rank_keys(&weights);
    let confidence_margin = match (candidates.first(), candidates.get(1)) {
        (Some(best), Some(runner_up)) => best.correlation - runner_up.correlation,
        _ => 0.0,
    };

    Some(TonalEstimate {
        candidates,
        confidence_margin,
    })
}

/// Resolves the weighting histogram: duration mass when any is present, else the
/// raw onset counts (KS v1 duration-only rule, onset fallback at zero duration).
#[allow(clippy::cast_precision_loss)]
fn resolve_weights(onset_counts: &[u32; 12], duration_mass: &[u64; 12]) -> [f64; 12] {
    let total_duration: u64 = duration_mass.iter().sum();
    let mut weights = [0.0_f64; 12];
    if total_duration == 0 {
        for (slot, &count) in weights.iter_mut().zip(onset_counts.iter()) {
            *slot = f64::from(count);
        }
    } else {
        // Duration mass in ticks; exact in f64 for any realistic score.
        for (slot, &mass) in weights.iter_mut().zip(duration_mass.iter()) {
            *slot = mass as f64;
        }
    }
    weights
}

/// Scores all 24 keys against `weights` and returns them best-first.
///
/// Ranking is a stable descending sort by correlation, so ties keep the
/// scan order (major before minor, tonic ascending). That reproduces
/// `estimate_harmony`'s strict-greater winner exactly and gives the full list a
/// deterministic order — over a flat histogram every key ties at zero and C
/// major sorts first.
#[allow(clippy::arithmetic_side_effects, clippy::indexing_slicing)]
fn rank_keys(weights: &[f64; 12]) -> Vec<TonalCandidate> {
    let total: f64 = weights.iter().sum();

    let mut candidates: Vec<TonalCandidate> = Vec::with_capacity(24);
    for mode in [KeyMode::Major, KeyMode::Minor] {
        let profile = mode.profile();
        for tonic in 0..12_u8 {
            let correlation = rotated_correlation(weights, profile, tonic);
            let on_scale: f64 = mode
                .scale_offsets()
                .iter()
                .map(|&offset| weights[usize::from((tonic + offset) % 12)])
                .sum();
            let scale_fit = if total > 0.0 { on_scale / total } else { 0.0 };
            candidates.push(TonalCandidate {
                tonic,
                mode,
                correlation,
                scale_fit,
            });
        }
    }

    candidates.sort_by(|a, b| {
        b.correlation
            .partial_cmp(&a.correlation)
            .unwrap_or(Ordering::Equal)
    });
    candidates
}

/// Pearson correlation between `histogram` and `profile` rotated so the
/// profile's tonic sits on pitch class `tonic`.
///
/// Returns a finite `0.0` when either side has zero variance (a flat histogram
/// correlates with nothing), so the score is always finite.
// Float-only arithmetic over fixed 12-bin arrays; indices are mod-12.
#[allow(clippy::arithmetic_side_effects, clippy::indexing_slicing)]
fn rotated_correlation(histogram: &[f64; 12], profile: &[f64; 12], tonic: u8) -> f64 {
    let mut rotated = [0.0_f64; 12];
    for (pc, slot) in rotated.iter_mut().enumerate() {
        *slot = profile[(pc + 12 - usize::from(tonic)) % 12];
    }

    let n = 12.0_f64;
    let mean_x: f64 = histogram.iter().sum::<f64>() / n;
    let mean_y: f64 = rotated.iter().sum::<f64>() / n;
    let mut numerator = 0.0_f64;
    let mut var_x = 0.0_f64;
    let mut var_y = 0.0_f64;
    for (x, y) in histogram.iter().zip(rotated.iter()) {
        let dx = x - mean_x;
        let dy = y - mean_y;
        numerator = dx.mul_add(dy, numerator);
        var_x = dx.mul_add(dx, var_x);
        var_y = dy.mul_add(dy, var_y);
    }
    let denominator = (var_x * var_y).sqrt();
    if denominator > 0.0 {
        numerator / denominator
    } else {
        0.0
    }
}

/// Mutable accumulator for [`PitchEvidence::measure`] — the running raw facts
/// as notes are folded in, one scope region at a time.
#[derive(Default)]
struct Tally {
    onset_counts: [u32; 12],
    duration_mass: [u64; 12],
    note_count: usize,
    pitch_range: Option<PitchRange>,
}

impl Tally {
    /// Folds every sounding note atom of `voice` into the tally, in stored order.
    fn visit_voice(&mut self, voice: &Voice) {
        for group in &voice.event_groups {
            for atom in &group.atoms {
                if let AtomEvent::Note(note) = atom {
                    self.push(note.pitch, note.duration.0);
                }
            }
        }
    }

    /// Folds one note's pitch and duration into the accumulators.
    #[allow(clippy::arithmetic_side_effects, clippy::indexing_slicing)]
    fn push(&mut self, pitch: Pitch, duration: u32) {
        let pc = usize::from(pitch.0) % 12;
        self.onset_counts[pc] = self.onset_counts[pc].saturating_add(1);
        self.duration_mass[pc] = self.duration_mass[pc].saturating_add(u64::from(duration));
        self.note_count = self.note_count.saturating_add(1);
        self.pitch_range = Some(self.pitch_range.map_or(
            PitchRange {
                lowest: pitch,
                highest: pitch,
            },
            |range| PitchRange {
                lowest: range.lowest.min(pitch),
                highest: range.highest.max(pitch),
            },
        ));
    }
}
