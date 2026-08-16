//! Shared pure-core tonal *evidence* and *inference* — the tonal-context layer
//! (S15 Phases 1–2).
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
//!
//! Phase 2 adds a third, *carriable* layer on top: a [`TonalContext`] bundles
//! the scope a caller chose, a compact [`TonalProjection`] of the ranked
//! estimate, and the [`TonalProvenance`] describing how it was measured, so a
//! generation request and its provenance can record which scope's estimate was
//! on the table. It is **carried, not consumed** — nothing here restricts a
//! pitch, reweights a policy, or changes a note.

use serde::{Deserialize, Serialize};
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
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
///
/// Serialises flat and tagged (`{"kind": "track", "track": 1}`) so a scope read
/// back out of an artifact says *which* region was measured without positional
/// guesswork — and so the shape is one plain object that can refuse unknown
/// fields. Serde's tagged-enum representations cannot: they accept foreign keys
/// silently, which would leave a hole at exactly the level a fail-closed
/// artifact must not have one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(into = "RawScope", try_from = "RawScope")]
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

    /// Checks that these facts are ones a measurement could have produced.
    ///
    /// [`measure`](Self::measure) satisfies every rule below by construction,
    /// but the fields are public and the type is deserialised at the
    /// [`TonalContext`] wire boundary, so evidence that arrives from anywhere
    /// else is *claimed*, not measured. Everything downstream — the estimate,
    /// the projection, the re-derivation that checks them — treats this
    /// structure as ground truth, which makes it the root of trust and the one
    /// place a forgery has to be stopped: an impossible fact set will otherwise
    /// yield an impeccably computed estimate and prove itself right about a lie.
    ///
    /// The estimator reads only the two histograms, and never reads them
    /// against each other or against the span, so none of these disagreements
    /// would surface later:
    ///
    /// - the span runs forwards;
    /// - `note_count` is the onset histogram's total;
    /// - a silent scope holds no duration mass and no span, and a sounding one
    ///   holds a span;
    /// - duration mass only where onsets are, and never more than those onsets
    ///   could carry (one onset contributes at most one `u32` of ticks);
    /// - the total duration mass fits `u64`;
    /// - every sounding pitch class has at least one MIDI representative inside
    ///   the observed span;
    /// - the span's endpoints are covered *per class, counting multiplicity*.
    ///   An endpoint is by definition an observed note, so each needs an onset
    ///   of its own: C4..C5 puts both ends in class C and therefore requires
    ///   **two** C onsets. Testing mere presence would let one note stand at
    ///   both ends of an octave, which no measurement can do.
    ///
    /// # Known limit
    /// [`measure`] tallies `onset_counts` as `u32` and `note_count` as `usize`,
    /// each saturating independently. Past `u32::MAX` notes in a single pitch
    /// class on a 64-bit target the two stop agreeing, and this validator would
    /// reject a genuine measurement on the `note_count`-versus-histogram rule.
    /// That threshold is unreachable for any real score — four billion notes of
    /// one pitch class — and it is Phase-1 behaviour, left alone deliberately
    /// rather than reworked for an astronomical case. The
    /// [`measure`]-and-`validate` proptest does not reach it either, so the
    /// agreement between the two is established below that bound, not above it.
    ///
    /// [`measure`]: Self::measure
    ///
    /// # Errors
    /// One [`TonalArtifactError`] naming the first rule broken.
    #[allow(clippy::indexing_slicing)] // fixed 12-bin histograms, indices are mod-12
    pub fn validate(&self) -> Result<(), TonalArtifactError> {
        if let Some(range) = self.pitch_range {
            if range.lowest > range.highest {
                return Err(TonalArtifactError::PitchRangeInverted {
                    lowest: range.lowest.0,
                    highest: range.highest.0,
                });
            }
        }

        let onsets = checked_total(self.onset_counts.iter().copied().map(u64::from))
            .ok_or(TonalArtifactError::OnsetCountOverflow)?;
        if u64::try_from(self.note_count).unwrap_or(u64::MAX) != onsets {
            return Err(TonalArtifactError::EvidenceNotSelfConsistent {
                note_count: self.note_count,
                onsets,
            });
        }
        if self.pitch_range.is_some() != (self.note_count > 0) {
            return Err(TonalArtifactError::EvidenceRangeMismatch {
                note_count: self.note_count,
                has_range: self.pitch_range.is_some(),
            });
        }

        for pc in 0..12_usize {
            let class_onsets = self.onset_counts[pc];
            let mass = self.duration_mass[pc];
            #[allow(clippy::cast_possible_truncation)] // pc < 12
            let pitch_class = pc as u8;
            if mass > 0 && class_onsets == 0 {
                return Err(TonalArtifactError::DurationMassWithoutOnsets { pitch_class });
            }
            // Both operands come from `u32`s, so the `u128` product is exact;
            // `saturating_mul` states that rather than relying on it.
            let ceiling = u128::from(class_onsets).saturating_mul(u128::from(u32::MAX));
            if u128::from(mass) > ceiling {
                return Err(TonalArtifactError::DurationMassExceedsOnsets { pitch_class });
            }
        }
        checked_total(self.duration_mass.iter().copied())
            .ok_or(TonalArtifactError::DurationMassOverflow)?;

        // Without a span there is nothing left to reconcile: the checks above
        // already established that such a scope is silent.
        let Some(range) = self.pitch_range else {
            return Ok(());
        };
        for pc in 0..12_usize {
            #[allow(clippy::cast_possible_truncation)] // pc < 12
            let pitch_class = pc as u8;
            if self.onset_counts[pc] > 0 && !range_holds_class(range, pitch_class) {
                return Err(TonalArtifactError::PitchClassOutsideObservedRange { pitch_class });
            }
        }
        // Each endpoint is a note that actually sounded, so each needs an onset
        // of its own. Counting per class rather than testing presence is what
        // makes an octave span (C4..C5, both ends in class C) demand *two* C
        // onsets — one onset cannot be at both ends at once.
        let mut required = [0_u32; 12];
        let lowest = usize::from(range.lowest.0 % 12);
        required[lowest] = required[lowest].saturating_add(1);
        if range.highest != range.lowest {
            let highest = usize::from(range.highest.0 % 12);
            required[highest] = required[highest].saturating_add(1);
        }
        for (pc, (&needed, &sounded)) in required.iter().zip(self.onset_counts.iter()).enumerate() {
            if sounded < needed {
                #[allow(clippy::cast_possible_truncation)] // pc < 12
                let pitch_class = pc as u8;
                return Err(TonalArtifactError::RangeEndpointsExceedOnsets {
                    pitch_class,
                    required: needed,
                    onsets: sounded,
                });
            }
        }

        Ok(())
    }
}

/// Sums without wrapping or panicking — `None` when the total leaves `u64`.
///
/// Untrusted histograms are summed through here rather than `Iterator::sum`,
/// which panics on overflow in a debug build and wraps in a release one. A
/// fail-closed artifact may do neither.
fn checked_total(mut values: impl Iterator<Item = u64>) -> Option<u64> {
    values.try_fold(0_u64, u64::checked_add)
}

/// Whether any MIDI pitch inside `range` belongs to `pitch_class`.
///
/// The lowest candidate at or above `range.lowest` is `lowest` advanced to the
/// next occurrence of the class; the class is representable exactly when that
/// candidate still fits under `highest`.
fn range_holds_class(range: PitchRange, pitch_class: u8) -> bool {
    let lowest = u16::from(range.lowest.0);
    // `pitch_class` and `lowest % 12` are both below 12, so the intermediate
    // stays well inside `u16`; saturating operations keep that explicit.
    let step = u16::from(pitch_class)
        .saturating_add(12)
        .saturating_sub(lowest % 12)
        % 12;
    lowest.saturating_add(step) <= u16::from(range.highest.0)
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

/// The estimator behind a [`TonalProjection`] — the *method* half of
/// [`TonalProvenance`].
///
/// Spelled out rather than implied: a projection read back from an artifact
/// must say which estimator produced it, and one naming a method this build
/// does not know refuses to deserialise instead of being read as the current
/// one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TonalMethod {
    /// Krumhansl–Schmuckler v1, as [`estimate_key`] implements it: Pearson
    /// correlation against the 24 rotated Krumhansl–Kessler profiles.
    KsV1,
}

/// Which histogram actually weighted an inference.
///
/// KS v1 is duration-only *except* that a scope whose notes all carry zero
/// duration falls back to raw onset counts, so the estimate stays defined.
/// Which of the two ran is invisible in the result, so the context records it —
/// otherwise a consumer cannot tell an estimate resting on sounded time from
/// one resting on bare attack tallies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TonalWeighting {
    /// Summed sounded ticks per pitch class — the normal path.
    DurationMass,
    /// Raw onset tallies — the fallback, taken only at zero total duration mass.
    OnsetCounts,
}

/// How a [`TonalProjection`] was measured.
///
/// Fields are private and there is no public constructor: a provenance record
/// is only ever produced by the estimator that it describes, or by a
/// [`TonalContext`] deserialisation that re-derived and checked it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TonalProvenance {
    method: TonalMethod,
    weighting: Option<TonalWeighting>,
}

impl TonalProvenance {
    /// The estimator that ran.
    #[must_use]
    pub const fn method(self) -> TonalMethod {
        self.method
    }

    /// The histogram that weighted it, or `None` when no estimate ran at all —
    /// a silent scope weights nothing, and saying otherwise would be a claim
    /// about material that was never there.
    #[must_use]
    pub const fn weighting(self) -> Option<TonalWeighting> {
        self.weighting
    }
}

/// The compact, immutable projection of a ranked [`TonalEstimate`]: its winner,
/// its closest rival, and the margin between them.
///
/// Compact by intent, and compact *safely* because a [`TonalContext`] carries
/// the [`PitchEvidence`] the estimate was drawn from: the other 22 candidates
/// are recoverable by handing that evidence back to [`estimate_key`], with no
/// score and no second measurement pass. What the projection must not drop is
/// the **uncertainty**: the runner-up and the margin stay, so a near tie still
/// reads as a near tie.
///
/// There is deliberately no `is_confident` and no threshold. Which margin counts
/// as confident is calibration work (S15 Phase 3B), not a constant.
///
/// Fields are private and the only constructor is
/// [`from_estimate`](Self::from_estimate), so a projection always is the top of
/// some ranking rather than three numbers someone assembled.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TonalProjection {
    winner: TonalCandidate,
    runner_up: Option<TonalCandidate>,
    confidence_margin: f64,
}

impl TonalProjection {
    /// Projects `estimate`, or `None` when it ranked nothing.
    #[must_use]
    pub fn from_estimate(estimate: &TonalEstimate) -> Option<Self> {
        Some(Self {
            winner: *estimate.winner()?,
            runner_up: estimate.candidates.get(1).copied(),
            confidence_margin: estimate.confidence_margin,
        })
    }

    /// The best-ranked candidate.
    #[must_use]
    pub const fn winner(self) -> TonalCandidate {
        self.winner
    }

    /// The next-best candidate, or `None` when the estimate ranked only one.
    #[must_use]
    pub const fn runner_up(self) -> Option<TonalCandidate> {
        self.runner_up
    }

    /// `winner.correlation - runner_up.correlation` (`0.0` when tied or alone).
    #[must_use]
    pub const fn confidence_margin(self) -> f64 {
        self.confidence_margin
    }
}

/// An explicitly scoped tonal estimate a caller may attach to a generation
/// request, and that the request's provenance carries back unchanged.
///
/// Three things travel together:
///
/// - the [`PitchEvidence`] of the scope the caller **chose** — there is no
///   scope-free constructor here, so nothing picks a track, a voice, or the
///   whole score on a caller's behalf (an S15 guardrail: `argmax(margin)` is
///   not an approved policy);
/// - the [`TonalProjection`], or `None` when the scope had nothing to estimate
///   from;
/// - the [`TonalProvenance`] saying how it was measured.
///
/// **Self-contained replay.** The evidence travels, not just the scope, so the
/// full 24-key ranking is recoverable from the artifact *alone*
/// (`estimate_key(context.evidence())`) — no score, no re-measurement, no "if
/// you still happen to hold the same input". That is what licenses the compact
/// projection; carrying only a scope would have made the claim depend on data
/// the artifact did not keep.
///
/// **Two absences, never conflated.** No context at all (an `Option` on the
/// request) means the caller never asked. A context with `projection: None`
/// means the caller asked about a scope that turned out silent — an honest
/// abstention, and itself a fact worth recording.
///
/// **The artifact proves itself.** Fields are private, and deserialisation goes
/// through one fail-closed wire form that re-derives the whole context from the
/// carried evidence and compares it against what the document claims, refusing
/// with a typed [`TonalArtifactError`] on any disagreement (the ADR-0033
/// "replay and compare, do not trust" posture). `deny_unknown_fields` guards
/// field *names*; this guards their *values*, which is where a provenance
/// record would otherwise be free to misdescribe its own origin.
///
/// **Carried, not consumed (Phase 2).** Generation, pitch selection, reranking,
/// and cadence are unchanged: a pass given a context produces byte-identical
/// output to one given none.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(into = "RawContext", try_from = "RawContext")]
pub struct TonalContext {
    evidence: PitchEvidence,
    projection: Option<TonalProjection>,
    provenance: TonalProvenance,
}

impl TonalContext {
    /// Measures `scope` on `score` and projects the estimate.
    ///
    /// The caller names the scope; this never searches for a better one.
    // `score` (the Score) and `scope` (the region) are distinct domain terms.
    #[allow(clippy::similar_names)]
    #[must_use]
    pub fn measure(score: &Score, scope: EvidenceScope) -> Self {
        // Measured evidence is valid by construction, so this door needs no
        // gate — `PitchEvidence::validate`'s own rules are exactly the ones
        // `measure` establishes, and a proptest holds the two together.
        Self::project(&PitchEvidence::measure(score, scope))
    }

    /// Projects already-measured `evidence`, keeping the scope it was measured
    /// over.
    ///
    /// The same contract as [`measure`](Self::measure) for a caller that
    /// already holds the evidence: the context's scope is the evidence's scope,
    /// never a second choice. Because [`PitchEvidence`] has public fields, the
    /// evidence handed in here is *claimed* rather than measured, so it goes
    /// through [`PitchEvidence::validate`] first — the same gate the wire
    /// boundary uses, so both doors admit exactly the same facts.
    ///
    /// # Errors
    /// [`TonalArtifactError`] when `evidence` is not something a measurement
    /// could have produced.
    pub fn from_evidence(evidence: &PitchEvidence) -> Result<Self, TonalArtifactError> {
        evidence.validate()?;
        Ok(Self::project(evidence))
    }

    /// The projection rule itself, over evidence already known to be valid.
    fn project(evidence: &PitchEvidence) -> Self {
        // One decision drives both fields, so a context can never claim to have
        // weighted a histogram it produced no projection from.
        let (projection, weighting) = estimate_with_weighting(
            evidence.note_count,
            &evidence.onset_counts,
            &evidence.duration_mass,
        )
        .and_then(|(estimate, weighting)| {
            TonalProjection::from_estimate(&estimate).map(|p| (Some(p), Some(weighting)))
        })
        .unwrap_or((None, None));

        Self {
            evidence: *evidence,
            projection,
            provenance: TonalProvenance {
                method: TonalMethod::KsV1,
                weighting,
            },
        }
    }

    /// The region the caller named.
    #[must_use]
    pub const fn scope(&self) -> EvidenceScope {
        self.evidence.scope
    }

    /// The raw evidence the estimate was drawn from — hand it to
    /// [`estimate_key`] to recover the full 24-key ranking.
    #[must_use]
    pub const fn evidence(&self) -> &PitchEvidence {
        &self.evidence
    }

    /// The compact ranked-estimate projection, or `None` for a silent scope.
    #[must_use]
    pub const fn projection(&self) -> Option<TonalProjection> {
        self.projection
    }

    /// How the estimate was measured.
    #[must_use]
    pub const fn provenance(&self) -> TonalProvenance {
        self.provenance
    }
}

// ── the artifact's wire form ─────────────────────────────────────────────────
//
// A [`TonalContext`] has exactly one serialised representation, defined by the
// private `Raw*` types below. Two properties follow from keeping it in one
// place, and neither survives a set of derived `Deserialize`s on the public
// types:
//
//   - every level denies unknown fields, including the scope (serde's tagged
//     enum representations accept foreign keys silently, so the scope is a flat
//     object here);
//   - every value is checked. Names are guarded by `deny_unknown_fields`;
//     *values* are guarded by re-deriving the whole context from the evidence
//     the document carries and comparing. A document that describes an estimate
//     its own evidence does not yield is refused, not read.
//
// The checks below the re-derivation are redundant with it by construction —
// the comparison would catch them all. They exist so the refusal names the
// actual fault instead of "does not match its own evidence".

/// Why a serialised [`TonalContext`] was refused.
///
/// Every variant is a *typed refusal*: the artifact is never repaired, coerced,
/// or partially accepted.
#[derive(Debug, Clone, Copy, PartialEq, thiserror::Error)]
pub enum TonalArtifactError {
    /// The scope's `kind` does not match the fields it carries.
    #[error("scope kind `{kind}` does not match its payload")]
    ScopeShape {
        /// The `kind` that was named.
        kind: &'static str,
    },
    /// A pitch outside the MIDI 7-bit range.
    #[error("pitch {value} is outside the MIDI range")]
    PitchOutOfRange {
        /// The offending value.
        value: u8,
    },
    /// The observed span runs backwards.
    #[error("pitch range {lowest}..{highest} is inverted")]
    PitchRangeInverted {
        /// Claimed lowest pitch.
        lowest: u8,
        /// Claimed highest pitch.
        highest: u8,
    },
    /// `note_count` disagrees with the onset histogram it is supposed to total.
    #[error("note_count {note_count} disagrees with the onset histogram ({onsets})")]
    EvidenceNotSelfConsistent {
        /// Claimed note count.
        note_count: usize,
        /// What the histogram actually sums to.
        onsets: u64,
    },
    /// A sounding scope with no observed span, or a silent one carrying a span.
    #[error("note_count {note_count} and pitch-range presence {has_range} disagree")]
    EvidenceRangeMismatch {
        /// Claimed note count.
        note_count: usize,
        /// Whether a span was carried.
        has_range: bool,
    },
    /// The onset histogram's total does not fit `u64`.
    #[error("the onset histogram does not sum within u64")]
    OnsetCountOverflow,
    /// A pitch class holds sounded ticks without ever having sounded.
    #[error("pitch class {pitch_class} holds duration mass but no onset")]
    DurationMassWithoutOnsets {
        /// The offending class.
        pitch_class: u8,
    },
    /// A pitch class holds more sounded ticks than its onsets could carry —
    /// each onset contributes at most one `u32` of ticks.
    #[error("pitch class {pitch_class} holds more duration mass than its onsets could")]
    DurationMassExceedsOnsets {
        /// The offending class.
        pitch_class: u8,
    },
    /// The duration histogram's total does not fit `u64`.
    #[error("the duration histogram does not sum within u64")]
    DurationMassOverflow,
    /// A sounding pitch class has no MIDI representative inside the observed
    /// span, so the span cannot be the one those notes were observed in.
    #[error("pitch class {pitch_class} sounded but fits nowhere in the observed range")]
    PitchClassOutsideObservedRange {
        /// The offending class.
        pitch_class: u8,
    },
    /// The span's endpoints need more onsets in one pitch class than the
    /// histogram holds — each endpoint is an observed note, and two endpoints
    /// sharing a class (an octave span) cannot rest on the same single onset.
    #[error(
        "the span endpoints need {required} onset(s) in pitch class {pitch_class}, \
         but {onsets} sounded"
    )]
    RangeEndpointsExceedOnsets {
        /// The under-supplied class.
        pitch_class: u8,
        /// How many onsets the endpoints require there.
        required: u32,
        /// How many the histogram holds.
        onsets: u32,
    },
    /// A tonic outside `0..=11`.
    #[error("tonic {tonic} is not a pitch class")]
    TonicOutOfRange {
        /// The offending value.
        tonic: u8,
    },
    /// A `scale_fit` outside `[0, 1]`, or not finite.
    #[error("scale_fit {scale_fit} is not a fraction in [0, 1]")]
    ScaleFitOutOfRange {
        /// The offending value.
        scale_fit: f64,
    },
    /// A correlation that is not a finite number.
    #[error("correlation {correlation} is not finite")]
    NonFiniteCorrelation {
        /// The offending value.
        correlation: f64,
    },
    /// A margin that is not finite, or is negative — the winner cannot trail
    /// its own runner-up.
    #[error("confidence margin {margin} is not a finite, non-negative gap")]
    MarginNotAFiniteGap {
        /// The offending value.
        margin: f64,
    },
    /// The margin is not the gap between the two candidates it travels with.
    #[error("confidence margin {claimed} is not the winner-rival gap {actual}")]
    MarginIsNotTheGap {
        /// The margin the document claims.
        claimed: f64,
        /// The gap its own candidates imply.
        actual: f64,
    },
    /// The projection, the weighting, and the note count disagree about whether
    /// an estimate happened at all.
    #[error(
        "projection presence {has_projection}, weighting presence {has_weighting}, \
         and note_count {note_count} disagree about whether an estimate ran"
    )]
    IncoherentAbsence {
        /// Whether a projection was carried.
        has_projection: bool,
        /// Whether a weighting was named.
        has_weighting: bool,
        /// Claimed note count.
        note_count: usize,
    },
    /// The document describes an estimate that its own evidence does not yield.
    #[error("the context does not match the evidence it carries")]
    DoesNotMatchItsOwnEvidence,
}

/// The `kind` discriminant of a serialised [`EvidenceScope`].
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum RawScopeKind {
    WholeScore,
    Track,
    Voice,
}

impl RawScopeKind {
    /// The wire spelling, for refusal messages.
    const fn label(self) -> &'static str {
        match self {
            Self::WholeScore => "whole_score",
            Self::Track => "track",
            Self::Voice => "voice",
        }
    }
}

/// A scope as one flat object: `{"kind": "voice", "track": 1, "voice": 0}`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawScope {
    kind: RawScopeKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    track: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    voice: Option<usize>,
}

impl From<EvidenceScope> for RawScope {
    fn from(scope: EvidenceScope) -> Self {
        match scope {
            EvidenceScope::WholeScore => Self {
                kind: RawScopeKind::WholeScore,
                track: None,
                voice: None,
            },
            EvidenceScope::Track(index) => Self {
                kind: RawScopeKind::Track,
                track: Some(index),
                voice: None,
            },
            EvidenceScope::Voice { track, voice } => Self {
                kind: RawScopeKind::Voice,
                track: Some(track),
                voice: Some(voice),
            },
        }
    }
}

impl TryFrom<RawScope> for EvidenceScope {
    type Error = TonalArtifactError;

    /// Each kind admits exactly one payload shape; anything else is refused
    /// rather than read with the surplus ignored.
    fn try_from(raw: RawScope) -> Result<Self, Self::Error> {
        match (raw.kind, raw.track, raw.voice) {
            (RawScopeKind::WholeScore, None, None) => Ok(Self::WholeScore),
            (RawScopeKind::Track, Some(track), None) => Ok(Self::Track(track)),
            (RawScopeKind::Voice, Some(track), Some(voice)) => Ok(Self::Voice { track, voice }),
            (kind, _, _) => Err(TonalArtifactError::ScopeShape { kind: kind.label() }),
        }
    }
}

/// An observed pitch span on the wire.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawPitchRange {
    lowest: u8,
    highest: u8,
}

/// The raw evidence the estimate was drawn from — the half of the artifact that
/// makes the ranking recoverable without the score.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawEvidence {
    scope: EvidenceScope,
    note_count: usize,
    onset_counts: [u32; 12],
    duration_mass: [u64; 12],
    pitch_range: Option<RawPitchRange>,
}

impl RawEvidence {
    /// Rebuilds the evidence and puts it through the shared validator.
    ///
    /// Only the raw-`u8` concern lives here: a value outside the MIDI range
    /// cannot become a [`Pitch`] at all, so it would never reach
    /// [`PitchEvidence::validate`]. Every other rule belongs to that one
    /// validator, so the wire boundary and [`TonalContext::from_evidence`]
    /// cannot drift into admitting different facts.
    fn into_evidence(self) -> Result<PitchEvidence, TonalArtifactError> {
        let pitch_range = self
            .pitch_range
            .map(|range| {
                let pitch = |value: u8| {
                    Pitch::new(value).map_err(|_| TonalArtifactError::PitchOutOfRange { value })
                };
                Ok(PitchRange {
                    lowest: pitch(range.lowest)?,
                    highest: pitch(range.highest)?,
                })
            })
            .transpose()?;

        let evidence = PitchEvidence {
            scope: self.scope,
            note_count: self.note_count,
            onset_counts: self.onset_counts,
            duration_mass: self.duration_mass,
            pitch_range,
        };
        evidence.validate()?;
        Ok(evidence)
    }
}

/// One ranked candidate on the wire.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCandidate {
    tonic: u8,
    mode: KeyMode,
    correlation: f64,
    scale_fit: f64,
}

impl From<TonalCandidate> for RawCandidate {
    fn from(candidate: TonalCandidate) -> Self {
        Self {
            tonic: candidate.tonic,
            mode: candidate.mode,
            correlation: candidate.correlation,
            scale_fit: candidate.scale_fit,
        }
    }
}

impl TryFrom<RawCandidate> for TonalCandidate {
    type Error = TonalArtifactError;

    fn try_from(raw: RawCandidate) -> Result<Self, Self::Error> {
        if raw.tonic > 11 {
            return Err(TonalArtifactError::TonicOutOfRange { tonic: raw.tonic });
        }
        if !raw.correlation.is_finite() {
            return Err(TonalArtifactError::NonFiniteCorrelation {
                correlation: raw.correlation,
            });
        }
        if !raw.scale_fit.is_finite() || !(0.0..=1.0).contains(&raw.scale_fit) {
            return Err(TonalArtifactError::ScaleFitOutOfRange {
                scale_fit: raw.scale_fit,
            });
        }
        Ok(Self {
            tonic: raw.tonic,
            mode: raw.mode,
            correlation: raw.correlation,
            scale_fit: raw.scale_fit,
        })
    }
}

/// The compact projection on the wire.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawProjection {
    winner: RawCandidate,
    runner_up: Option<RawCandidate>,
    confidence_margin: f64,
}

impl From<TonalProjection> for RawProjection {
    fn from(projection: TonalProjection) -> Self {
        Self {
            winner: projection.winner.into(),
            runner_up: projection.runner_up.map(Into::into),
            confidence_margin: projection.confidence_margin,
        }
    }
}

impl TryFrom<RawProjection> for TonalProjection {
    type Error = TonalArtifactError;

    /// The margin is re-derived from the two candidates it travels with, so a
    /// document cannot overstate its own confidence while carrying the
    /// candidates that contradict it.
    // Exact float comparison is the contract, not an oversight: the margin is a
    // single subtraction of the document's own two correlations, so a tolerance
    // here would be a licence to misstate confidence by that tolerance.
    #[allow(clippy::arithmetic_side_effects, clippy::float_cmp)]
    fn try_from(raw: RawProjection) -> Result<Self, Self::Error> {
        let winner = TonalCandidate::try_from(raw.winner)?;
        let runner_up = raw.runner_up.map(TonalCandidate::try_from).transpose()?;

        let margin = raw.confidence_margin;
        if !margin.is_finite() || margin < 0.0 {
            return Err(TonalArtifactError::MarginNotAFiniteGap { margin });
        }
        let actual = runner_up.map_or(0.0, |rival| winner.correlation - rival.correlation);
        if margin != actual {
            return Err(TonalArtifactError::MarginIsNotTheGap {
                claimed: margin,
                actual,
            });
        }

        Ok(Self {
            winner,
            runner_up,
            confidence_margin: margin,
        })
    }
}

/// The provenance on the wire.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawProvenance {
    method: TonalMethod,
    weighting: Option<TonalWeighting>,
}

/// The whole artifact on the wire.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawContext {
    evidence: RawEvidence,
    projection: Option<RawProjection>,
    provenance: RawProvenance,
}

impl From<TonalContext> for RawContext {
    fn from(context: TonalContext) -> Self {
        let evidence = context.evidence;
        Self {
            evidence: RawEvidence {
                scope: evidence.scope,
                note_count: evidence.note_count,
                onset_counts: evidence.onset_counts,
                duration_mass: evidence.duration_mass,
                pitch_range: evidence.pitch_range.map(|range| RawPitchRange {
                    lowest: range.lowest.0,
                    highest: range.highest.0,
                }),
            },
            projection: context.projection.map(Into::into),
            provenance: RawProvenance {
                method: context.provenance.method,
                weighting: context.provenance.weighting,
            },
        }
    }
}

impl TryFrom<RawContext> for TonalContext {
    type Error = TonalArtifactError;

    /// Replays the artifact instead of trusting it: the evidence must be
    /// self-consistent, the projection well-formed, the absences coherent, and
    /// then the whole context is **re-derived from that evidence and compared**.
    /// What comes back is the re-derived value, so a caller always holds a
    /// context its own evidence supports.
    ///
    /// The float comparison is exact. Every operation in the estimator
    /// (multiply, add, `mul_add`, `sqrt`, divide) is IEEE-754 correctly rounded
    /// and `serde_json` round-trips `f64` losslessly, which is the same
    /// determinism the S6 chain baseline's aggregate-bit golden already rests
    /// on.
    fn try_from(raw: RawContext) -> Result<Self, Self::Error> {
        let evidence = raw.evidence.into_evidence()?;
        let projection = raw.projection.map(TonalProjection::try_from).transpose()?;

        let has_projection = projection.is_some();
        let has_weighting = raw.provenance.weighting.is_some();
        if has_projection != has_weighting || has_projection != (evidence.note_count > 0) {
            return Err(TonalArtifactError::IncoherentAbsence {
                has_projection,
                has_weighting,
                note_count: evidence.note_count,
            });
        }

        let claimed = Self {
            evidence,
            projection,
            provenance: TonalProvenance {
                method: raw.provenance.method,
                weighting: raw.provenance.weighting,
            },
        };
        // `into_evidence` already ran the validator, so the projection rule is
        // applied directly rather than re-checking facts already established.
        let derived = Self::project(&evidence);
        if derived == claimed {
            Ok(derived)
        } else {
            Err(TonalArtifactError::DoesNotMatchItsOwnEvidence)
        }
    }
}

/// The shared inference core over raw histograms, reused by [`estimate_key`] and
/// by `complement::estimate_harmony` (which has weighted notes, not a scope).
pub(crate) fn estimate_from_histograms(
    note_count: usize,
    onset_counts: &[u32; 12],
    duration_mass: &[u64; 12],
) -> Option<TonalEstimate> {
    estimate_with_weighting(note_count, onset_counts, duration_mass).map(|(estimate, _)| estimate)
}

/// The same inference, also reporting *which* histogram weighted it — the one
/// place that decides, so [`TonalContext`]'s provenance and the estimate itself
/// can never disagree about the KS v1 fallback.
#[allow(clippy::arithmetic_side_effects, clippy::indexing_slicing)]
pub(crate) fn estimate_with_weighting(
    note_count: usize,
    onset_counts: &[u32; 12],
    duration_mass: &[u64; 12],
) -> Option<(TonalEstimate, TonalWeighting)> {
    if note_count == 0 {
        return None;
    }

    let (weights, weighting) = resolve_weights(onset_counts, duration_mass);
    let candidates = rank_keys(&weights);
    let confidence_margin = match (candidates.first(), candidates.get(1)) {
        (Some(best), Some(runner_up)) => best.correlation - runner_up.correlation,
        _ => 0.0,
    };

    Some((
        TonalEstimate {
            candidates,
            confidence_margin,
        },
        weighting,
    ))
}

/// Resolves the weighting histogram: duration mass when any is present, else the
/// raw onset counts (KS v1 duration-only rule, onset fallback at zero duration).
///
/// Returns the branch it took alongside the weights, so the fallback is a
/// reportable fact rather than an invisible one.
#[allow(clippy::cast_precision_loss)]
fn resolve_weights(
    onset_counts: &[u32; 12],
    duration_mass: &[u64; 12],
) -> ([f64; 12], TonalWeighting) {
    // Saturating, not `sum()`: `estimate_key` is public over a `PitchEvidence`
    // with public fields, so the histogram is not guaranteed summable. Which
    // branch is taken only depends on the total being non-zero, and saturation
    // preserves that, so no measured score's estimate changes.
    let total_duration: u64 = duration_mass.iter().copied().fold(0, u64::saturating_add);
    let mut weights = [0.0_f64; 12];
    if total_duration == 0 {
        for (slot, &count) in weights.iter_mut().zip(onset_counts.iter()) {
            *slot = f64::from(count);
        }
        (weights, TonalWeighting::OnsetCounts)
    } else {
        // Duration mass in ticks; exact in f64 for any realistic score.
        for (slot, &mass) in weights.iter_mut().zip(duration_mass.iter()) {
            *slot = mass as f64;
        }
        (weights, TonalWeighting::DurationMass)
    }
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
