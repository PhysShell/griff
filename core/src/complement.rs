//! `ComplementArranger` — rule-based complementary-part generation (S13).
//!
//! Given an existing part A on a [`Score`], `ComplementArranger` analyses A into a
//! [`PartProfile`], picks a [`RelationMode`], *derives* part B, and appends it as
//! a new [`Track`] on A's shared [`crate::score::MasterBar`] timeline (ADR-0003).
//! It compiles a relative intent into the canonical model rather than being a new
//! generation core (ADR-0012); modes that synthesise fresh pitch sequences will
//! delegate to the S6 generator ([`crate::generate::generate`]).
//!
//! All six relation modes are implemented, each on its defining axis:
//!
//! - `rhythm_lock` — B on A's exact onset grid, pitches substituted from A's
//!   harmony in a shifted band (reuses A's onsets directly, no S6 round-trip).
//! - `register_contrast` — the same grid lock, but the shifted band must stay
//!   **disjoint** from A's after MIDI clamping ([`ComplementError::InvalidSpec`]
//!   otherwise).
//! - `support_layer` — one root-pedal note per non-empty bar at A's first
//!   onset; strictly sparser than A wherever A plays more than one note a bar.
//! - `call_response` — B answers in A's gaps (the onset complement); a part
//!   with no answerable gap is [`ComplementError::NoGapsToAnswer`].
//! - `octave_double` — A's contour copied a non-zero whole number of octaves
//!   away, marks preserved.
//! - `counter_melody` — an independent line delegated to the S6
//!   `ConstrainedRandomWalk` over a request derived from A; needs a uniform
//!   master-bar timeline ([`ComplementError::NonUniformTimeline`]).

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::event::{NoteMark, NoteMarks, Pitch, SpanTechnique, Ticks, Tuning, Velocity};
use crate::feature::PitchRange;
use crate::fretboard::{
    measure_playability, FingeringWeights, PlayabilityReport, STANDARD_MAX_FRET,
};
use crate::generate::{
    generate, GenerationConstraints, GenerationError, GenerationSeed, GenerationStrategy,
    PitchMaterial, RuleGenerationRequest,
};
use crate::score::{AtomEvent, AtomNote, EventGroup, EventGroupKind, Score, Track, Voice};
use crate::scoring::{rank_indices, Axes, Axis, Scored, WeightPolicy};

/// A named complementarity preset for generating part B (glossary §8).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum RelationMode {
    /// Lock B's rhythm to A's onset grid; substitute pitches from A's harmony.
    RhythmLock,
    /// Place B in a register band disjoint from A.
    RegisterContrast,
    /// B answers A in the gaps (call and response).
    CallResponse,
    /// A sparser low-register layer beneath A.
    SupportLayer,
    /// Reproduce A's contour an octave away.
    OctaveDouble,
    /// An independent counter-melody against A.
    CounterMelody,
}

impl RelationMode {
    /// Short stable label used in provenance and track naming.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::RhythmLock => "rhythm_lock",
            Self::RegisterContrast => "register_contrast",
            Self::CallResponse => "call_response",
            Self::SupportLayer => "support_layer",
            Self::OctaveDouble => "octave_double",
            Self::CounterMelody => "counter_melody",
        }
    }
}

/// Relation-as-spec: what the caller asks `ComplementArranger` to produce.
#[derive(Debug, Clone, Copy)]
pub struct ComplementSpec {
    /// Which relation mode to apply.
    pub mode: RelationMode,
    /// Semitone shift applied to B's register relative to A (e.g. `-12` = octave down).
    pub register_offset: i8,
}

/// Pitch-variability *ask*, orthogonal to [`RelationMode`] (ADR-0023).
///
/// The [`crate::gesture::GestureControl`] duality for the complement arranger:
/// it shapes B's pitch over a fixed rhythmic skeleton and never moves an onset,
/// so a grid-locked mode stays grid-locked.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VariationControl {
    /// Fraction of the band's scale ladder B may use, `0.0..=1.0`. `0.0` pins
    /// every note to the band's anchor degree (a static line, still locked to
    /// A's grid); `1.0` uses the whole band — the unconstrained default that
    /// matches [`arrange_complement`].
    pub pitch_spread: f64,
}

impl VariationControl {
    /// The identity control: the whole band, i.e. [`arrange_complement`]'s
    /// behaviour.
    pub const FULL: Self = Self { pitch_spread: 1.0 };
    /// A static line: B collapses onto the band's anchor degree.
    pub const LOCKED: Self = Self { pitch_spread: 0.0 };

    /// Whether the control is in range: a finite `pitch_spread` within
    /// `0.0..=1.0`.
    #[must_use]
    pub fn is_valid(self) -> bool {
        self.pitch_spread.is_finite() && (0.0..=1.0).contains(&self.pitch_spread)
    }
}

/// A varied complement: the arranged candidate plus provenance — the control
/// that asked for it (ask) and B's realised pitch spread (is).
#[derive(Debug, Clone)]
pub struct VariedComplement {
    /// The arranged candidate (A's score with B appended).
    pub complement: ComplementCandidate,
    /// The control this candidate was arranged against.
    pub control: VariationControl,
    /// B's realised pitch ambitus as a fraction of the target band,
    /// `0.0..=1.0` — what the spread actually *is*, not what was asked.
    pub realized_spread: f64,
}

/// Errors the varied arrangement entry point can emit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VariationError {
    /// The [`VariationControl`] is out of range (non-finite or outside
    /// `0.0..=1.0`).
    InvalidControl,
    /// The underlying arrangement failed.
    Arrange(ComplementError),
}

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
}

/// The part's estimated key and how well its notes fit that key's scale —
/// the harmonic context of a part profile (glossary §8).
///
/// Estimated with the Krumhansl–Schmuckler key-finding algorithm: the part's
/// duration-weighted pitch-class histogram is correlated (Pearson) against
/// the 24 rotated Krumhansl–Kessler tonal-hierarchy profiles and the best
/// correlation wins, ties resolving to the earliest key in the
/// major-then-minor, C-upward scan. `scale_fit` is a fact, not a verdict —
/// what counts as "fitting well enough" is corpus/S9 calibration territory.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HarmonicContext {
    /// Tonic pitch class: `0` = C … `11` = B.
    pub tonic_pitch_class: u8,
    /// Major or natural minor.
    pub mode: KeyMode,
    /// Duration-weighted fraction of notes on the key's scale, in `[0, 1]`.
    pub scale_fit: f64,
}

/// The per-part feature summary derived from part A (glossary §8).
///
/// Richer than `VoiceFeatures`: it is bucketed against the master-bar timeline
/// so the onset/rhythm grid is available per bar.
#[derive(Debug, Clone)]
pub struct PartProfile {
    /// Per master bar, the ordered note durations of the analysed voice — A's
    /// onset/rhythm grid.
    pub bar_rhythms: Vec<Vec<Ticks>>,
    /// Register band across all note atoms, or `None` when the part has no notes.
    pub register: Option<PitchRange>,
    /// Total note count.
    pub note_count: usize,
    /// Normalised density: notes per master bar.
    pub density: f64,
    /// Distinct MIDI pitches present, ascending.
    pub pitches: Vec<u8>,
    /// Distinct technique labels across the part — per-note marks *and* spanning
    /// techniques (ADR-0018), so the overlap axis sees both.
    pub techniques: BTreeSet<&'static str>,
    /// The estimated key and scale fit, or `None` when the part has no notes.
    pub harmony: Option<HarmonicContext>,
}

/// Relation-as-provenance: per-axis scores describing how B relates to A.
///
/// Comparable and serialisable since corpus schema v4, where measured pair
/// relations persist inside ensemble groups (decisions 2026-06-11).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AxisScores {
    /// Jaccard overlap of A's and B's onset sets, in `[0, 1]`: `1.0` for the
    /// grid-locked modes, `0.0` for the onset-complement (`call_response`).
    pub rhythm_similarity: f64,
    /// Fraction of register-band overlap between A and B, in `[0, 1]`.
    pub register_overlap: f64,
    /// `density(B) / density(A)`.
    pub density_ratio: f64,
    /// Jaccard overlap of the two technique sets, in `[0, 1]` (`1.0` if both empty).
    pub technique_overlap: f64,
}

// Stable relation-axis labels — the join keys between [`AxisScores`] facts and a
// scoring [`WeightPolicy`] (ADR-0017). Named once so the labels and the axis
// order cannot drift apart.
const AXIS_RHYTHM_SIMILARITY: &str = "rhythm_similarity";
const AXIS_REGISTER_OVERLAP: &str = "register_overlap";
const AXIS_DENSITY_RATIO: &str = "density_ratio";
const AXIS_TECHNIQUE_OVERLAP: &str = "technique_overlap";

/// The complement relation axes, in their canonical order (ADR-0017).
pub const RELATION_AXIS_LABELS: [&str; 4] = [
    AXIS_RHYTHM_SIMILARITY,
    AXIS_REGISTER_OVERLAP,
    AXIS_DENSITY_RATIO,
    AXIS_TECHNIQUE_OVERLAP,
];

impl AxisScores {
    /// The four relation axes as labelled scoring facts (ADR-0017).
    ///
    /// Exposes the existing per-axis measurements as shared [`Axes`] data, in
    /// [`RELATION_AXIS_LABELS`] order, so the complement relation is scored,
    /// ranked, and explained with the same vocabulary as every other score.
    #[must_use]
    pub fn axes(&self) -> Axes {
        Axes::new(vec![
            Axis {
                label: AXIS_RHYTHM_SIMILARITY,
                value: self.rhythm_similarity,
            },
            Axis {
                label: AXIS_REGISTER_OVERLAP,
                value: self.register_overlap,
            },
            Axis {
                label: AXIS_DENSITY_RATIO,
                value: self.density_ratio,
            },
            Axis {
                label: AXIS_TECHNIQUE_OVERLAP,
                value: self.technique_overlap,
            },
        ])
    }
}

/// The baseline relation weight policy (`relation` v1): uniform over the four
/// relation axes.
///
/// Untuned by design — weights are *data* the feedback layer (S9) learns
/// (ADR-0017 §3), and per-relation-mode policies are future data, not branches
/// hardcoded here. For the implemented `rhythm_lock` mode the uniform aggregate
/// reads as "how strongly B relates to A".
#[must_use]
pub fn relation_weights_v1() -> WeightPolicy {
    WeightPolicy::uniform("relation", 1, &RELATION_AXIS_LABELS)
}

/// A produced complement: the combined score plus provenance.
#[derive(Debug, Clone)]
pub struct ComplementCandidate {
    /// A's score with part B appended as a new track on the shared master bars.
    pub score: Score,
    /// Index of the appended part-B track in `score.tracks`.
    pub part_b_index: usize,
    /// The relation mode that produced B.
    pub mode: RelationMode,
    /// Seed used for this arrangement.
    pub seed: GenerationSeed,
    /// Per-axis relation scores.
    pub axis_scores: AxisScores,
}

impl ComplementCandidate {
    /// An explainable [`Scored`] view of this candidate under `policy`
    /// (ADR-0017).
    ///
    /// The value is the part-B track locator; the provenance carries the seed
    /// and the weight-policy version, so the aggregate and any ranking are
    /// reproducible relative to `(seed, policy version)` (ADR-0017 §7).
    #[must_use]
    pub fn scored(&self, policy: &WeightPolicy) -> Scored<usize> {
        Scored::new(
            self.part_b_index,
            self.axis_scores.axes(),
            policy,
            Some(self.seed.0),
        )
    }
}

/// Ranks complement candidates under `policy`, most-related first, ties broken
/// by candidate order (the fixed tie-break rule, ADR-0017 §7).
///
/// Returns indices into `candidates`. Deterministic for fixed inputs and a fixed
/// policy version.
#[must_use]
pub fn rank_candidates(candidates: &[ComplementCandidate], policy: &WeightPolicy) -> Vec<usize> {
    let scored: Vec<Scored<usize>> = candidates.iter().map(|c| c.scored(policy)).collect();
    rank_indices(&scored)
}

/// Result of the (A, B) pair validator.
#[derive(Debug, Clone, Copy)]
pub struct PairValidation {
    /// Number of dissonant intervals on coincident onsets (m2 / tritone / M7).
    pub coincident_dissonances: usize,
    /// Whether A and B occupy overlapping registers too heavily ("register mud").
    pub register_mud: bool,
    /// Playability of A's melodic line on the optimal fingering path under
    /// A's own tuning (the per-part filter, ADR-0019).
    pub a_playability: PlayabilityReport,
    /// Playability of B's melodic line, likewise under B's own tuning.
    pub b_playability: PlayabilityReport,
}

impl PairValidation {
    /// A pair is clean when there are no coincident dissonances, no register
    /// mud, and both parts are playable (every line note reachable on the
    /// fretboard). Fret travel stays a carried fact, not part of the verdict
    /// — jump thresholds are calibration data, not code.
    #[must_use]
    pub const fn is_clean(&self) -> bool {
        self.coincident_dissonances == 0
            && !self.register_mud
            && self.a_playability.is_playable()
            && self.b_playability.is_playable()
    }
}

/// Errors `ComplementArranger` can emit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ComplementError {
    /// The score has no master bars to arrange against.
    EmptyScore,
    /// The requested track index is out of range.
    TrackIndexOutOfRange,
    /// The requested part has no note atoms to analyse.
    PartHasNoNotes,
    /// The spec is incompatible with the mode's contract (e.g. `octave_double`
    /// with a `register_offset` that is not a non-zero whole octave).
    InvalidSpec(RelationMode),
    /// `call_response` found no gap in part A long enough to answer (at least
    /// one quarter note of silence after A's first sound).
    NoGapsToAnswer,
    /// `counter_melody` needs every master bar to share one meter and span:
    /// the S6 delegate lays bars back-to-back from a single time signature,
    /// which cannot align with a mid-score meter change.
    NonUniformTimeline,
    /// The underlying S6 generator rejected the derived request.
    Generation(GenerationError),
}

/// One note flattened to absolute position for analysis.
#[derive(Debug, Clone, Copy)]
struct NoteRef {
    onset: u32,
    duration: Ticks,
    pitch: u8,
    velocity: u8,
}

/// Collects every note atom of a track's primary voice, sorted by onset.
fn voice_notes(track: &Track) -> Vec<NoteRef> {
    let Some(voice) = track.voices.first() else {
        return Vec::new();
    };
    let mut notes: Vec<NoteRef> = voice
        .event_groups
        .iter()
        .flat_map(|g| &g.atoms)
        .filter_map(|a| match a {
            AtomEvent::Note(n) => Some(NoteRef {
                onset: n.absolute_start.0,
                duration: n.duration,
                pitch: n.pitch.0,
                velocity: n.velocity.0,
            }),
            AtomEvent::Rest(_) => None,
        })
        .collect();
    notes.sort_by_key(|n| n.onset);
    notes
}

/// Collects the technique labels in a track's primary voice — per-note
/// [`NoteMark`]s plus spanning [`TechniqueSpan`]s — so the `technique_overlap`
/// axis reflects both, not only marks (ADR-0018).
fn voice_technique_labels(track: &Track) -> BTreeSet<&'static str> {
    let mut labels: BTreeSet<&'static str> = BTreeSet::new();
    let Some(voice) = track.voices.first() else {
        return labels;
    };
    for group in &voice.event_groups {
        for atom in &group.atoms {
            if let AtomEvent::Note(n) = atom {
                for mark in n.marks.iter() {
                    labels.insert(note_mark_label(mark));
                }
            }
        }
        for span in &group.technique_spans {
            labels.insert(span_label(span.technique));
        }
    }
    labels
}

/// Stable label for a per-note mark.
const fn note_mark_label(mark: NoteMark) -> &'static str {
    match mark {
        NoteMark::Accent => "accent",
        NoteMark::Ghost => "ghost",
        NoteMark::Staccato => "staccato",
        NoteMark::DeadNote => "dead_note",
        NoteMark::HarmonicNatural => "harmonic_natural",
        NoteMark::HarmonicPinch => "harmonic_pinch",
        NoteMark::Tap => "tap",
    }
}

/// Stable label for a spanning technique.
const fn span_label(technique: SpanTechnique) -> &'static str {
    match technique {
        SpanTechnique::Slide => "slide",
        SpanTechnique::Bend => "bend",
        SpanTechnique::Legato => "legato",
        SpanTechnique::PalmMute => "palm_mute",
        SpanTechnique::HammerOn => "hammer_on",
        SpanTechnique::PullOff => "pull_off",
        SpanTechnique::Vibrato => "vibrato",
        SpanTechnique::LetRing => "let_ring",
    }
}

/// Analyses part A (the track at `track_index`) into a [`PartProfile`].
pub fn analyze_part(score: &Score, track_index: usize) -> Result<PartProfile, ComplementError> {
    let track = score
        .tracks
        .get(track_index)
        .ok_or(ComplementError::TrackIndexOutOfRange)?;

    let notes = voice_notes(track);

    // Bucket note durations into the master-bar grid, preserving onset order.
    let bar_rhythms: Vec<Vec<Ticks>> = score
        .master_bars
        .iter()
        .map(|mb| {
            notes
                .iter()
                .filter(|n| n.onset >= mb.tick_range.start.0 && n.onset < mb.tick_range.end.0)
                .map(|n| n.duration)
                .collect()
        })
        .collect();

    let register = notes
        .iter()
        .map(|n| n.pitch)
        .fold(None, |acc, p| match acc {
            None => Some((p, p)),
            Some((lo, hi)) => Some((lo.min(p), hi.max(p))),
        });
    let register = register.and_then(|(lo, hi)| {
        Some(PitchRange {
            lowest: Pitch::new(lo).ok()?,
            highest: Pitch::new(hi).ok()?,
        })
    });

    let mut pitches: Vec<u8> = notes.iter().map(|n| n.pitch).collect();
    pitches.sort_unstable();
    pitches.dedup();

    let techniques = voice_technique_labels(track);
    let weighted: Vec<(u8, u32)> = notes.iter().map(|n| (n.pitch, n.duration.0)).collect();
    let harmony = estimate_harmony(&weighted);

    let note_count = notes.len();
    let bar_count = score.master_bars.len();
    let density = density_per_bar(note_count, bar_count);

    Ok(PartProfile {
        bar_rhythms,
        register,
        note_count,
        density,
        pitches,
        techniques,
        harmony,
    })
}

/// Krumhansl–Kessler major tonal-hierarchy profile (probe-tone ratings).
const KK_MAJOR: [f64; 12] = [
    6.35, 2.23, 3.48, 2.33, 4.38, 4.09, 2.52, 5.19, 2.39, 3.66, 2.29, 2.88,
];
/// Krumhansl–Kessler natural-minor tonal-hierarchy profile.
const KK_MINOR: [f64; 12] = [
    6.33, 2.68, 3.52, 5.38, 2.60, 3.53, 2.54, 4.75, 3.98, 2.69, 3.34, 3.17,
];

/// Estimates the part's key from its notes (Krumhansl–Schmuckler), or `None`
/// when there are none.
///
/// Histogram weights are note durations in ticks; a part whose notes all have
/// zero duration falls back to counting notes so the estimate stays defined.
/// Takes `(pitch, duration-in-ticks)` pairs so other analyses (the S14
/// complexity profile) reuse the one estimator.
// Float-only arithmetic over fixed 12-bin histograms; pitch-class indices are
// mod-12 by construction.
#[allow(clippy::arithmetic_side_effects, clippy::indexing_slicing)]
pub(crate) fn estimate_harmony(notes: &[(u8, u32)]) -> Option<HarmonicContext> {
    if notes.is_empty() {
        return None;
    }

    let total_ticks: u64 = notes.iter().map(|&(_, d)| u64::from(d)).sum();
    let mut histogram = [0.0_f64; 12];
    for &(pitch, duration) in notes {
        let pc = usize::from(pitch) % 12;
        let weight = if total_ticks == 0 {
            1.0
        } else {
            f64::from(duration)
        };
        histogram[pc] += weight;
    }

    let mut best: Option<(f64, u8, KeyMode)> = None;
    for mode in [KeyMode::Major, KeyMode::Minor] {
        let profile = match mode {
            KeyMode::Major => &KK_MAJOR,
            KeyMode::Minor => &KK_MINOR,
        };
        for tonic in 0..12_u8 {
            let r = rotated_correlation(&histogram, profile, tonic);
            if best.map_or(true, |(b, _, _)| r > b) {
                best = Some((r, tonic, mode));
            }
        }
    }
    let (_, tonic_pitch_class, mode) = best?;

    let total: f64 = histogram.iter().sum();
    let on_scale: f64 = mode
        .scale_offsets()
        .iter()
        .map(|&s| histogram[usize::from(tonic_pitch_class + s) % 12])
        .sum();
    let scale_fit = if total > 0.0 { on_scale / total } else { 0.0 };

    Some(HarmonicContext {
        tonic_pitch_class,
        mode,
        scale_fit,
    })
}

/// Pearson correlation between `histogram` and `profile` rotated so the
/// profile's tonic sits on pitch class `tonic`.
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

/// Measures the complement relation between two *existing* tracks.
///
/// The corpus-side counterpart of the per-mode `AxisScores` provenance, used
/// to persist ensemble pair relations (corpus schema v4, decisions
/// 2026-06-11).
///
/// Orientation: the Jaccard axes (rhythm, technique) and the band overlap are
/// symmetric; `density_ratio` reads `track_b` relative to `track_a` — pass the
/// lower part index first. Deterministic for fixed inputs.
pub fn measure_pair_axes(
    score: &Score,
    track_a: usize,
    track_b: usize,
) -> Result<AxisScores, ComplementError> {
    let a = analyze_part(score, track_a)?;
    let b = analyze_part(score, track_b)?;
    if a.note_count == 0 || b.note_count == 0 {
        return Err(ComplementError::PartHasNoNotes);
    }

    let density_ratio = if a.density == 0.0 {
        0.0
    } else {
        b.density / a.density
    };
    let register_overlap = match (a.register, b.register) {
        (Some(ra), Some(rb)) => band_overlap(ra.lowest.0, ra.highest.0, rb.lowest.0, rb.highest.0),
        _ => 0.0,
    };
    let technique_overlap = jaccard(&a.techniques, &b.techniques);

    let onsets = |index: usize| -> BTreeSet<u32> {
        score.tracks.get(index).map_or_else(BTreeSet::new, |t| {
            voice_notes(t).iter().map(|n| n.onset).collect()
        })
    };
    let rhythm_similarity = jaccard(&onsets(track_a), &onsets(track_b));

    Ok(AxisScores {
        rhythm_similarity,
        register_overlap,
        density_ratio,
        technique_overlap,
    })
}

/// Notes per bar as a float; `0.0` when there are no bars.
#[allow(clippy::cast_precision_loss)]
fn density_per_bar(note_count: usize, bar_count: usize) -> f64 {
    if bar_count == 0 {
        0.0
    } else {
        note_count as f64 / bar_count as f64
    }
}

/// Shifts a MIDI pitch by `semitones`, clamping to the valid `0..=127` range.
fn shift_pitch(pitch: u8, semitones: i8) -> u8 {
    let raw = i32::from(pitch).saturating_add(i32::from(semitones));
    let clamped = raw.clamp(0, 127);
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    {
        clamped as u8
    }
}

/// Arranges a complementary part B from part A at `track_index`.
///
/// Returns a [`ComplementCandidate`] whose `score` is A's score with B appended
/// as a new track on the same master bars. Deterministic for a fixed
/// `(score, track_index, spec, seed)`.
///
/// Equivalent to [`arrange_complement_varied`] with [`VariationControl::FULL`]
/// (the whole-band pitch window).
pub fn arrange_complement(
    score: &Score,
    track_index: usize,
    spec: ComplementSpec,
    seed: GenerationSeed,
) -> Result<ComplementCandidate, ComplementError> {
    arrange_complement_inner(score, track_index, spec, seed, VariationControl::FULL)
}

/// Arranges a complement under a [`VariationControl`], with ask-vs-is provenance.
///
/// Returns the candidate plus the control and B's realised spread
/// ([`VariedComplement`]). The control shapes B's pitch only — onsets stay
/// locked to the chosen [`RelationMode`]'s skeleton — and an out-of-range
/// control is the typed [`VariationError::InvalidControl`].
///
/// Deterministic for a fixed `(score, track_index, spec, seed, control)`
/// (SPEC §6): the control narrows the seeded pitch hash, it does not add
/// randomness.
pub fn arrange_complement_varied(
    score: &Score,
    track_index: usize,
    spec: ComplementSpec,
    seed: GenerationSeed,
    control: VariationControl,
) -> Result<VariedComplement, VariationError> {
    if !control.is_valid() {
        return Err(VariationError::InvalidControl);
    }
    let complement = arrange_complement_inner(score, track_index, spec, seed, control)
        .map_err(VariationError::Arrange)?;
    let realized_spread = realized_band_spread(score, track_index, spec, &complement);
    Ok(VariedComplement {
        complement,
        control,
        realized_spread,
    })
}

/// Shared dispatch for both entry points: the ladder-substitution modes
/// (`rhythm_lock`, `register_contrast`, `call_response`) honour `control`'s
/// pitch spread; the others have no ladder to window and ignore it.
fn arrange_complement_inner(
    score: &Score,
    track_index: usize,
    spec: ComplementSpec,
    seed: GenerationSeed,
    control: VariationControl,
) -> Result<ComplementCandidate, ComplementError> {
    if score.master_bars.is_empty() {
        return Err(ComplementError::EmptyScore);
    }
    let profile = analyze_part(score, track_index)?;
    if profile.note_count == 0 {
        return Err(ComplementError::PartHasNoNotes);
    }

    match spec.mode {
        RelationMode::RhythmLock => {
            arrange_rhythm_lock(score, track_index, &profile, spec, seed, control)
        }
        RelationMode::RegisterContrast => {
            arrange_register_contrast(score, track_index, &profile, spec, seed, control)
        }
        RelationMode::SupportLayer => {
            arrange_support_layer(score, track_index, &profile, spec, seed)
        }
        RelationMode::CallResponse => {
            arrange_call_response(score, track_index, &profile, spec, seed, control)
        }
        RelationMode::OctaveDouble => {
            arrange_octave_double(score, track_index, &profile, spec, seed)
        }
        RelationMode::CounterMelody => {
            arrange_counter_melody(score, track_index, &profile, spec, seed)
        }
    }
}

/// Measures B's realised pitch spread: its ambitus as a fraction of the target
/// band `[band_lo, band_hi]` (the `shifted_band` A's register maps to). `0.0`
/// when B is a single pitch, a degenerate band, or has no notes.
fn realized_band_spread(
    score: &Score,
    track_index: usize,
    spec: ComplementSpec,
    complement: &ComplementCandidate,
) -> f64 {
    let Ok(profile) = analyze_part(score, track_index) else {
        return 0.0;
    };
    let Some(register) = profile.register else {
        return 0.0;
    };
    let (band_lo, band_hi) = shifted_band(register, spec.register_offset);
    let span = band_hi.saturating_sub(band_lo);
    if span == 0 {
        return 0.0;
    }
    let b_notes = complement
        .score
        .tracks
        .get(complement.part_b_index)
        .map_or_else(Vec::new, voice_notes);
    let (Some(lo), Some(hi)) = (
        b_notes.iter().map(|n| n.pitch).min(),
        b_notes.iter().map(|n| n.pitch).max(),
    ) else {
        return 0.0;
    };
    f64::from(hi.saturating_sub(lo)) / f64::from(span)
}

/// `counter_melody`: an independent line against A, delegated to the S6
/// generator's `ConstrainedRandomWalk` — the one mode that synthesises a fresh
/// sequence instead of deriving B from A's grid.
///
/// The request is compiled from A: A's harmonic context as the scale, A's band
/// shifted by `spec.register_offset` as the pitch bounds, A's bar count /
/// meter / tempo / PPQN as constraints, A's bar rhythms as templates. The
/// generated line is lifted onto A's master bars, which therefore must share
/// one meter and span ([`ComplementError::NonUniformTimeline`] otherwise — S6
/// lays bars back-to-back from a single time signature).
fn arrange_counter_melody(
    score: &Score,
    track_index: usize,
    profile: &PartProfile,
    spec: ComplementSpec,
    seed: GenerationSeed,
) -> Result<ComplementCandidate, ComplementError> {
    let register = profile.register.ok_or(ComplementError::PartHasNoNotes)?;
    let (band_lo, band_hi) = shifted_band(register, spec.register_offset);

    let first = score
        .master_bars
        .first()
        .ok_or(ComplementError::EmptyScore)?;
    let first_span = first
        .tick_range
        .end
        .0
        .saturating_sub(first.tick_range.start.0);
    let uniform = score.master_bars.iter().all(|mb| {
        mb.time_signature == first.time_signature
            && mb.tick_range.end.0.saturating_sub(mb.tick_range.start.0) == first_span
    });
    if !uniform {
        return Err(ComplementError::NonUniformTimeline);
    }

    let request = RuleGenerationRequest {
        seed,
        pitch_material: PitchMaterial {
            root: Pitch::new(band_lo).unwrap_or(register.lowest),
            intervals: scale_intervals_from(profile, band_lo),
        },
        constraints: GenerationConstraints {
            bar_count: score.master_bars.len(),
            time_signature: first.time_signature,
            tempo: first.tempo,
            ticks_per_quarter: Ticks(u32::from(score.ticks_per_quarter)),
            pitch_lo: Pitch::new(band_lo).unwrap_or(register.lowest),
            pitch_hi: Pitch::new(band_hi).unwrap_or(register.highest),
        },
        source_rhythms: profile.bar_rhythms.clone(),
        strategy: GenerationStrategy::ConstrainedRandomWalk,
    };
    let candidate = generate(&request).map_err(ComplementError::Generation)?;

    // Lift the generated line onto A's timeline (S6 lays bars from tick 0).
    let offset = first.tick_range.start.0;
    let event_groups: Vec<EventGroup> = candidate
        .score
        .tracks
        .first()
        .into_iter()
        .flat_map(|t| &t.voices)
        .flat_map(|v| &v.event_groups)
        .flat_map(|g| &g.atoms)
        .filter_map(|a| match a {
            AtomEvent::Note(n) => {
                let mut out = *n;
                out.absolute_start = Ticks(n.absolute_start.0.saturating_add(offset));
                Some(EventGroup {
                    kind: EventGroupKind::Single,
                    atoms: vec![AtomEvent::Note(out)],
                    technique_spans: Vec::new(),
                })
            }
            AtomEvent::Rest(_) => None,
        })
        .collect();

    Ok(finish_candidate(
        score,
        track_index,
        profile,
        spec.mode,
        seed,
        event_groups,
        Pitch::new(band_lo).unwrap_or(register.lowest),
        Pitch::new(band_hi).unwrap_or(register.highest),
    ))
}

/// `octave_double`: reproduce A's contour a whole octave (or several) away —
/// every B note copies A's onset, duration, velocity, and marks, with the
/// pitch shifted by `spec.register_offset` and clamped to the MIDI range.
///
/// The offset must be a non-zero whole-octave shift; anything else is an
/// [`ComplementError::InvalidSpec`] — a third-doubling is a different relation,
/// not a sloppy octave. Purely analytic: the seed is recorded as provenance
/// but draws nothing.
fn arrange_octave_double(
    score: &Score,
    track_index: usize,
    profile: &PartProfile,
    spec: ComplementSpec,
    seed: GenerationSeed,
) -> Result<ComplementCandidate, ComplementError> {
    if spec.register_offset == 0 || spec.register_offset.checked_rem(12) != Some(0) {
        return Err(ComplementError::InvalidSpec(RelationMode::OctaveDouble));
    }
    let register = profile.register.ok_or(ComplementError::PartHasNoNotes)?;
    let a_track = score
        .tracks
        .get(track_index)
        .ok_or(ComplementError::TrackIndexOutOfRange)?;

    let event_groups: Vec<EventGroup> = voice_atom_notes(a_track)
        .iter()
        .map(|n| {
            let mut out = *n;
            out.pitch = Pitch(shift_pitch(n.pitch.0, spec.register_offset));
            // A shifted pitch invalidates any carried fretboard position.
            out.position = None;
            EventGroup {
                kind: EventGroupKind::Single,
                atoms: vec![AtomEvent::Note(out)],
                technique_spans: Vec::new(),
            }
        })
        .collect();

    let b_lo = shift_pitch(register.lowest.0, spec.register_offset);
    let b_hi = shift_pitch(register.highest.0, spec.register_offset);
    Ok(finish_candidate(
        score,
        track_index,
        profile,
        spec.mode,
        seed,
        event_groups,
        Pitch(b_lo.min(b_hi)),
        Pitch(b_lo.max(b_hi)),
    ))
}

/// `rhythm_lock`: place B on A's *actual* per-bar onset grid — every B note
/// keeps A's onset and duration, with the pitch substituted from A's harmony
/// shifted into the register requested by `spec.register_offset`.
///
/// Because B reuses A's onsets directly (rather than a regenerated grid), the
/// rhythm is locked exactly even when A has rests, off-beat starts, different
/// rhythms in later bars, or per-bar meter changes — A's onsets already respect
/// A's master-bar timeline. Pitch selection is seed-deterministic.
#[allow(clippy::too_many_arguments)] // a ladder mode threading the variation control
fn arrange_rhythm_lock(
    score: &Score,
    track_index: usize,
    profile: &PartProfile,
    spec: ComplementSpec,
    seed: GenerationSeed,
    control: VariationControl,
) -> Result<ComplementCandidate, ComplementError> {
    // Register band for B: A's band shifted, clamped, and ordered.
    let register = profile.register.ok_or(ComplementError::PartHasNoNotes)?;
    let (band_lo, band_hi) = shifted_band(register, spec.register_offset);

    let a_track = score
        .tracks
        .get(track_index)
        .ok_or(ComplementError::TrackIndexOutOfRange)?;
    let event_groups = grid_locked_groups(a_track, profile, band_lo, band_hi, seed, control);

    Ok(finish_candidate(
        score,
        track_index,
        profile,
        spec.mode,
        seed,
        event_groups,
        Pitch::new(band_lo).unwrap_or(register.lowest),
        Pitch::new(band_hi).unwrap_or(register.highest),
    ))
}

/// `register_contrast`: B on A's exact onset grid, but in a register band
/// **disjoint** from A's — A's band shifted by `spec.register_offset`.
///
/// If the shifted band still intersects A's after MIDI clamping (including a
/// zero offset, or a large shift folded back by the clamp), the contrast
/// contract cannot be met and the spec is rejected as
/// [`ComplementError::InvalidSpec`] rather than silently overlapped.
#[allow(clippy::too_many_arguments)] // a ladder mode threading the variation control
fn arrange_register_contrast(
    score: &Score,
    track_index: usize,
    profile: &PartProfile,
    spec: ComplementSpec,
    seed: GenerationSeed,
    control: VariationControl,
) -> Result<ComplementCandidate, ComplementError> {
    let register = profile.register.ok_or(ComplementError::PartHasNoNotes)?;
    let (band_lo, band_hi) = shifted_band(register, spec.register_offset);

    // Closed intervals [a_lo, a_hi] and [band_lo, band_hi] intersect iff each
    // starts before the other ends.
    if register.lowest.0 <= band_hi && band_lo <= register.highest.0 {
        return Err(ComplementError::InvalidSpec(RelationMode::RegisterContrast));
    }

    let a_track = score
        .tracks
        .get(track_index)
        .ok_or(ComplementError::TrackIndexOutOfRange)?;
    let event_groups = grid_locked_groups(a_track, profile, band_lo, band_hi, seed, control);

    Ok(finish_candidate(
        score,
        track_index,
        profile,
        spec.mode,
        seed,
        event_groups,
        Pitch::new(band_lo).unwrap_or(register.lowest),
        Pitch::new(band_hi).unwrap_or(register.highest),
    ))
}

/// `support_layer`: a sparser pedal layer beneath A — one note per non-empty
/// master bar, placed at A's first onset in that bar with that note's duration
/// and velocity, pitched at A's lowest pitch shifted by
/// `spec.register_offset` (a root pedal; typically an octave down).
///
/// Bars where A is silent get no pedal, so `density(B) < density(A)` whenever
/// A plays more than one note in any bar. Purely analytic: the seed is
/// provenance only.
fn arrange_support_layer(
    score: &Score,
    track_index: usize,
    profile: &PartProfile,
    spec: ComplementSpec,
    seed: GenerationSeed,
) -> Result<ComplementCandidate, ComplementError> {
    let register = profile.register.ok_or(ComplementError::PartHasNoNotes)?;
    let pedal = shift_pitch(register.lowest.0, spec.register_offset);
    let a_track = score
        .tracks
        .get(track_index)
        .ok_or(ComplementError::TrackIndexOutOfRange)?;
    let a_notes = voice_notes(a_track);

    let event_groups: Vec<EventGroup> = score
        .master_bars
        .iter()
        .filter_map(|mb| {
            let first = a_notes
                .iter()
                .find(|n| n.onset >= mb.tick_range.start.0 && n.onset < mb.tick_range.end.0)?;
            // `first.velocity` originates from a valid AtomNote, so it is in range.
            Some(EventGroup {
                kind: EventGroupKind::Single,
                atoms: vec![AtomEvent::Note(AtomNote {
                    absolute_start: Ticks(first.onset),
                    duration: first.duration,
                    pitch: Pitch::new(pedal).unwrap_or(register.lowest),
                    velocity: Velocity::new(first.velocity).unwrap_or(Velocity(0)),
                    marks: NoteMarks::empty(),
                    position: None,
                })],
                technique_spans: Vec::new(),
            })
        })
        .collect();

    Ok(finish_candidate(
        score,
        track_index,
        profile,
        spec.mode,
        seed,
        event_groups,
        Pitch::new(pedal).unwrap_or(register.lowest),
        Pitch::new(pedal).unwrap_or(register.lowest),
    ))
}

/// `call_response`: B answers A in its gaps — the onset complement.
///
/// A gap is a maximal silent span of A's merged note coverage, between A's
/// first sound and the end of the last master bar (leading silence has no call
/// to answer). Every gap at least one quarter long gets exactly one answer: a
/// B note at the gap start sustaining through the gap, at the velocity of the
/// preceding A note, pitched seed-deterministically from A's harmonic context in
/// the band shifted by `spec.register_offset`. B's onsets are disjoint from
/// A's by construction. No qualifying gap is
/// [`ComplementError::NoGapsToAnswer`].
#[allow(clippy::too_many_arguments)] // a ladder mode threading the variation control
fn arrange_call_response(
    score: &Score,
    track_index: usize,
    profile: &PartProfile,
    spec: ComplementSpec,
    seed: GenerationSeed,
    control: VariationControl,
) -> Result<ComplementCandidate, ComplementError> {
    let register = profile.register.ok_or(ComplementError::PartHasNoNotes)?;
    let (band_lo, band_hi) = shifted_band(register, spec.register_offset);
    let a_track = score
        .tracks
        .get(track_index)
        .ok_or(ComplementError::TrackIndexOutOfRange)?;
    let a_notes = voice_notes(a_track);

    // Merge A's coverage intervals (notes are sorted by onset).
    let mut coverage: Vec<(u32, u32)> = Vec::new();
    for n in &a_notes {
        let end = n.onset.saturating_add(n.duration.0);
        match coverage.last_mut() {
            Some((_, last_end)) if n.onset <= *last_end => *last_end = (*last_end).max(end),
            _ => coverage.push((n.onset, end)),
        }
    }

    // Gaps: between merged spans, plus the trailing gap to the span end.
    let span_end = score.master_bars.last().map_or(0, |mb| mb.tick_range.end.0);
    let mut gaps: Vec<(u32, u32)> = coverage
        .windows(2)
        .filter_map(|w| match (w.first(), w.get(1)) {
            (Some(&(_, end)), Some(&(next_start, _))) if next_start > end => {
                Some((end, next_start))
            }
            _ => None,
        })
        .collect();
    if let Some(&(_, last_end)) = coverage.last() {
        if span_end > last_end {
            gaps.push((last_end, span_end));
        }
    }

    let min_gap = u32::from(score.ticks_per_quarter);
    let scale = scale_intervals_from(profile, band_lo);
    let ladder = band_scale_ladder(band_lo, band_hi, &scale);
    let window = spread_window(ladder.len(), control.pitch_spread);
    let event_groups: Vec<EventGroup> = gaps
        .iter()
        .filter(|(start, end)| end.saturating_sub(*start) >= min_gap)
        .enumerate()
        .map(|(i, &(start, end))| {
            let ladder_index = pitch_index(seed.0, i, window);
            let pitch_val = ladder.get(ladder_index).copied().unwrap_or(band_lo);
            // The call this gap answers: the last A note sounding before it.
            let call_velocity = a_notes
                .iter()
                .rev()
                .find(|n| n.onset < start)
                .map_or(90, |n| n.velocity);
            EventGroup {
                kind: EventGroupKind::Single,
                atoms: vec![AtomEvent::Note(AtomNote {
                    absolute_start: Ticks(start),
                    duration: Ticks(end.saturating_sub(start)),
                    pitch: Pitch::new(pitch_val).unwrap_or(Pitch(band_lo)),
                    velocity: Velocity::new(call_velocity).unwrap_or(Velocity(0)),
                    marks: NoteMarks::empty(),
                    position: None,
                })],
                technique_spans: Vec::new(),
            }
        })
        .collect();

    if event_groups.is_empty() {
        return Err(ComplementError::NoGapsToAnswer);
    }

    Ok(finish_candidate(
        score,
        track_index,
        profile,
        spec.mode,
        seed,
        event_groups,
        Pitch::new(band_lo).unwrap_or(register.lowest),
        Pitch::new(band_hi).unwrap_or(register.highest),
    ))
}

/// A's register band shifted by `offset` semitones, MIDI-clamped and ordered.
fn shifted_band(register: PitchRange, offset: i8) -> (u8, u8) {
    let lo = shift_pitch(register.lowest.0, offset);
    let hi = shift_pitch(register.highest.0, offset);
    (lo.min(hi), lo.max(hi))
}

/// B's event groups on A's exact onset grid: every B note keeps A's onset,
/// duration, and velocity, with the pitch substituted seed-deterministically
/// from A's harmonic context mapped into the `[band_lo, band_hi]` register band.
#[allow(clippy::too_many_arguments)] // shared grid builder threading the variation control
fn grid_locked_groups(
    a_track: &Track,
    profile: &PartProfile,
    band_lo: u8,
    band_hi: u8,
    seed: GenerationSeed,
    control: VariationControl,
) -> Vec<EventGroup> {
    // Scale: A's harmonic context, as intervals above the band's low note,
    // anchored to A's pitch classes (the band picks the octave, not the key).
    let intervals = scale_intervals_from(profile, band_lo);
    let ladder = band_scale_ladder(band_lo, band_hi, &intervals);
    // Pitch spread narrows how much of the ladder B may wander across.
    let window = spread_window(ladder.len(), control.pitch_spread);

    voice_notes(a_track)
        .iter()
        .enumerate()
        .map(|(i, n)| {
            let ladder_index = pitch_index(seed.0, i, window);
            let pitch_val = ladder.get(ladder_index).copied().unwrap_or(band_lo);
            let pitch = Pitch::new(pitch_val).unwrap_or(Pitch(band_lo));
            // `n.velocity` originates from a valid AtomNote, so it is always in range.
            let velocity = Velocity::new(n.velocity).unwrap_or(Velocity(0));
            EventGroup {
                kind: EventGroupKind::Single,
                atoms: vec![AtomEvent::Note(AtomNote {
                    absolute_start: Ticks(n.onset),
                    duration: n.duration,
                    pitch,
                    velocity,
                    marks: NoteMarks::empty(),
                    position: None,
                })],
                technique_spans: Vec::new(),
            }
        })
        .collect()
}

/// Assembles the produced part B into a [`ComplementCandidate`]: appends the
/// event groups as a new track on A's master bars (channel after A's, standard
/// tuning — positions are not derived yet) and computes the axis provenance
/// against the `[b_lo, b_hi]` register band the mode targeted.
#[allow(clippy::too_many_arguments)] // a private assembly seam shared by every mode
fn finish_candidate(
    score: &Score,
    track_index: usize,
    profile: &PartProfile,
    mode: RelationMode,
    seed: GenerationSeed,
    event_groups: Vec<EventGroup>,
    b_lo: Pitch,
    b_hi: Pitch,
) -> ComplementCandidate {
    let a_channel = score.tracks.get(track_index).map_or(0, |t| t.channel);
    let b_channel = if a_channel >= 15 {
        0
    } else {
        a_channel.saturating_add(1)
    };

    let b_track = Track {
        name: Some(format!("Complement ({})", mode.label())),
        channel: b_channel,
        voices: vec![Voice {
            id: 0,
            event_groups,
        }],
        tuning: Tuning::standard_e(),
    };

    let mut combined = score.clone();
    combined.tracks.push(b_track);
    let part_b_index = combined.tracks.len().saturating_sub(1);

    let axis_scores = score_axes(profile, &combined, track_index, part_b_index, b_lo, b_hi);

    ComplementCandidate {
        score: combined,
        part_b_index,
        mode,
        seed,
        axis_scores,
    }
}

/// Collects every note atom of a track's primary voice as full [`AtomNote`]s
/// (marks included), sorted by onset — for modes that copy A's notes verbatim.
fn voice_atom_notes(track: &Track) -> Vec<AtomNote> {
    let Some(voice) = track.voices.first() else {
        return Vec::new();
    };
    let mut notes: Vec<AtomNote> = voice
        .event_groups
        .iter()
        .flat_map(|g| &g.atoms)
        .filter_map(|a| match a {
            AtomEvent::Note(n) => Some(*n),
            AtomEvent::Rest(_) => None,
        })
        .collect();
    notes.sort_by_key(|n| n.absolute_start.0);
    notes
}

/// The scale B substitutes pitches from, as semitone offsets above `band_lo`
/// (mod 12), sorted and non-empty: A's distinct pitch classes *plus* the
/// inferred key's scale (the harmonic context — enrich, don't replace), so B
/// can always echo A's actual notes and a sparse part still opens a whole
/// scale of material.
///
/// Offsets are measured from the *band floor's* pitch class — consumers apply
/// them at `band_lo`, so the materialised pitch classes equal A's regardless
/// of the register offset: the key is the anchor, the band only picks the
/// octave. Measuring from A's lowest pitch instead transposed the whole
/// material with a non-octave shift (a C-major part offset a fifth grew an
/// F#; Codex P2, PR #38).
fn scale_intervals_from(profile: &PartProfile, band_lo: u8) -> Vec<u8> {
    let anchor_pc = band_lo.checked_rem(12).unwrap_or(0);
    let interval_above_anchor = |pc: u8| -> u8 {
        pc.saturating_add(12)
            .saturating_sub(anchor_pc)
            .checked_rem(12)
            .unwrap_or(0)
    };

    let mut intervals: Vec<u8> = profile
        .pitches
        .iter()
        .map(|&p| interval_above_anchor(p.checked_rem(12).unwrap_or(0)))
        .collect();
    if let Some(harmony) = profile.harmony {
        for offset in harmony.mode.scale_offsets() {
            let pc = harmony
                .tonic_pitch_class
                .saturating_add(offset)
                .checked_rem(12)
                .unwrap_or(0);
            intervals.push(interval_above_anchor(pc));
        }
    }
    intervals.sort_unstable();
    intervals.dedup();
    if intervals.is_empty() {
        intervals.push(0);
    }
    intervals
}

/// The number of bottom ladder entries B may wander across under a pitch-spread
/// control: `pitch_spread` scales the `ladder_len`, rounded, floored at 1 so B
/// always has at least the anchor degree, capped at the full ladder.
///
/// `pitch_spread = 1.0` returns `ladder_len` (the whole band — the identity
/// window that reproduces the unconstrained arranger); `0.0` returns `1` (every
/// pick collapses onto degree 0). `ladder_len == 0` returns `0` (an empty
/// ladder, handled by the caller's `unwrap_or`).
fn spread_window(ladder_len: usize, pitch_spread: f64) -> usize {
    if ladder_len == 0 {
        return 0;
    }
    let spread = pitch_spread.clamp(0.0, 1.0);
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )] // ladder_len ≤ MIDI range; result clamped to [1, ladder_len]
    let window = (spread * ladder_len as f64).round() as usize;
    window.clamp(1, ladder_len)
}

/// Every on-scale pitch within `[lo, hi]`, across all octaves, ascending — the
/// full ladder B draws from.
///
/// `intervals` are mod-12 offsets above `lo`'s pitch class, so each octave
/// repeats the same shape. Spanning the whole band (not just `lo..=lo + 11`) is
/// what lets B inhabit A's real register instead of collapsing into the bottom
/// octave — every degree (`ladder_index`) was previously placed only in the
/// lowest octave.
fn band_scale_ladder(lo: u8, hi: u8, intervals: &[u8]) -> Vec<u8> {
    let hi16 = u16::from(hi);
    let mut ladder: Vec<u8> = Vec::new();
    let mut base = u16::from(lo);
    while base <= hi16 {
        for &interval in intervals {
            let p = base.saturating_add(u16::from(interval));
            if p <= hi16 {
                if let Ok(p8) = u8::try_from(p) {
                    ladder.push(p8);
                }
            }
        }
        base = base.saturating_add(12);
    }
    ladder.sort_unstable();
    ladder.dedup();
    if ladder.is_empty() {
        ladder.push(lo);
    }
    ladder
}

/// Seed-deterministic scale-degree (`ladder_index`) picker for note `index`
/// (`SplitMix64` finalizer).
fn pitch_index(seed: u64, index: usize, modulo: usize) -> usize {
    if modulo == 0 {
        return 0;
    }
    let salt = u64::try_from(index)
        .unwrap_or(0)
        .wrapping_mul(0x9E37_79B9_7F4A_7C15);
    let mut z = seed.wrapping_add(salt);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    let m = u64::try_from(modulo).unwrap_or(1).max(1);
    usize::try_from(z.checked_rem(m).unwrap_or(0)).unwrap_or(0)
}

/// Computes per-axis provenance scores for the produced pair.
#[allow(clippy::too_many_arguments)] // a private provenance seam shared by every mode
fn score_axes(
    profile: &PartProfile,
    combined: &Score,
    a_index: usize,
    part_b_index: usize,
    b_lo: Pitch,
    b_hi: Pitch,
) -> AxisScores {
    let b_notes = combined
        .tracks
        .get(part_b_index)
        .map_or_else(Vec::new, voice_notes);
    let b_density = density_per_bar(b_notes.len(), combined.master_bars.len());

    let density_ratio = if profile.density == 0.0 {
        0.0
    } else {
        b_density / profile.density
    };

    let register_overlap = profile.register.map_or(0.0, |a| {
        band_overlap(a.lowest.0, a.highest.0, b_lo.0, b_hi.0)
    });

    let b_techniques = combined
        .tracks
        .get(part_b_index)
        .map(voice_technique_labels)
        .unwrap_or_default();
    let technique_overlap = jaccard(&profile.techniques, &b_techniques);

    // Onset-set Jaccard: exactly 1.0 for the grid-locked modes (B reuses A's
    // onsets), the shared fraction for sparse / gap-filling modes.
    let a_onsets: BTreeSet<u32> = combined
        .tracks
        .get(a_index)
        .map_or_else(BTreeSet::new, |t| {
            voice_notes(t).iter().map(|n| n.onset).collect()
        });
    let b_onsets: BTreeSet<u32> = b_notes.iter().map(|n| n.onset).collect();
    let rhythm_similarity = jaccard(&a_onsets, &b_onsets);

    AxisScores {
        rhythm_similarity,
        register_overlap,
        density_ratio,
        technique_overlap,
    }
}

/// Overlap fraction of two pitch bands, relative to the narrower band.
#[allow(clippy::cast_precision_loss)]
fn band_overlap(a_lo: u8, a_hi: u8, b_lo: u8, b_hi: u8) -> f64 {
    let lo = a_lo.max(b_lo);
    let hi = a_hi.min(b_hi);
    let overlap = u32::from(hi.saturating_sub(lo));
    let a_span = u32::from(a_hi.saturating_sub(a_lo));
    let b_span = u32::from(b_hi.saturating_sub(b_lo));
    let narrower = a_span.min(b_span);
    if hi < lo {
        return 0.0;
    }
    if narrower == 0 {
        // Degenerate (single-pitch) band: overlapping iff the point lies inside.
        return if hi >= lo { 1.0 } else { 0.0 };
    }
    f64::from(overlap) / f64::from(narrower)
}

/// Jaccard overlap of two sets; `1.0` when both are empty.
#[allow(clippy::cast_precision_loss)]
fn jaccard<T: Ord>(a: &BTreeSet<T>, b: &BTreeSet<T>) -> f64 {
    let inter = a.intersection(b).count();
    let union = a.union(b).count();
    if union == 0 {
        1.0
    } else {
        inter as f64 / union as f64
    }
}

/// Dissonant interval classes (mod 12): minor second, tritone, major seventh.
const DISSONANT_CLASSES: [u8; 3] = [1, 6, 11];

/// Validates the (A, B) pair: coincident-onset dissonances, register mud,
/// and per-part playability.
///
/// Dissonances are counted on coincident onsets; register mud flags the two
/// parts overlapping the same register too heavily. Playability measures
/// each part's melodic line (the shared highest-pitch-per-onset convention)
/// on the optimal fingering path under the part's own tuning, with the `v1`
/// weights and the standard fret range (ADR-0019). Chord voicing
/// playability stays deferred (ADR-0019 §7): a chord participates through
/// its top note only.
pub fn validate_pair(
    score: &Score,
    a_index: usize,
    b_index: usize,
) -> Result<PairValidation, ComplementError> {
    let a = score
        .tracks
        .get(a_index)
        .ok_or(ComplementError::TrackIndexOutOfRange)?;
    let b = score
        .tracks
        .get(b_index)
        .ok_or(ComplementError::TrackIndexOutOfRange)?;

    let a_notes = voice_notes(a);
    let b_notes = voice_notes(b);

    let mut coincident_dissonances: usize = 0;
    for an in &a_notes {
        for bn in b_notes.iter().filter(|bn| bn.onset == an.onset) {
            let hi = an.pitch.max(bn.pitch);
            let lo = an.pitch.min(bn.pitch);
            let interval = hi.saturating_sub(lo).checked_rem(12).unwrap_or(0);
            if DISSONANT_CLASSES.contains(&interval) {
                coincident_dissonances = coincident_dissonances.saturating_add(1);
            }
        }
    }

    let register_mud = match (band_of(&a_notes), band_of(&b_notes)) {
        (Some((al, ah)), Some((bl, bh))) => band_overlap(al, ah, bl, bh) > 0.5,
        _ => false,
    };

    Ok(PairValidation {
        coincident_dissonances,
        register_mud,
        a_playability: part_playability(&a_notes, &a.tuning),
        b_playability: part_playability(&b_notes, &b.tuning),
    })
}

/// Playability of one part's melodic line: folds the onset-sorted notes to
/// the highest pitch per onset (the closure / novelty / gesture line
/// convention) and measures the optimal fingering path under `tuning`.
fn part_playability(notes: &[NoteRef], tuning: &Tuning) -> PlayabilityReport {
    let mut line: Vec<(u32, u8)> = Vec::new();
    for n in notes {
        match line.last_mut() {
            Some((onset, pitch)) if *onset == n.onset => *pitch = (*pitch).max(n.pitch),
            _ => line.push((n.onset, n.pitch)),
        }
    }
    // NoteRef pitches originate from valid AtomNotes, so they are in range.
    let pitches: Vec<Pitch> = line.into_iter().map(|(_, p)| Pitch(p)).collect();
    measure_playability(&pitches, tuning, &FingeringWeights::v1(), STANDARD_MAX_FRET)
}

/// Lowest/highest pitch across notes, or `None` when empty.
fn band_of(notes: &[NoteRef]) -> Option<(u8, u8)> {
    notes
        .iter()
        .map(|n| n.pitch)
        .fold(None, |acc, p| match acc {
            None => Some((p, p)),
            Some((lo, hi)) => Some((lo.min(p), hi.max(p))),
        })
}
