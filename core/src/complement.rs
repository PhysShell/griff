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
    })
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
pub fn arrange_complement(
    score: &Score,
    track_index: usize,
    spec: ComplementSpec,
    seed: GenerationSeed,
) -> Result<ComplementCandidate, ComplementError> {
    if score.master_bars.is_empty() {
        return Err(ComplementError::EmptyScore);
    }
    let profile = analyze_part(score, track_index)?;
    if profile.note_count == 0 {
        return Err(ComplementError::PartHasNoNotes);
    }

    match spec.mode {
        RelationMode::RhythmLock => arrange_rhythm_lock(score, track_index, &profile, spec, seed),
        RelationMode::RegisterContrast => {
            arrange_register_contrast(score, track_index, &profile, spec, seed)
        }
        RelationMode::SupportLayer => {
            arrange_support_layer(score, track_index, &profile, spec, seed)
        }
        RelationMode::CallResponse => {
            arrange_call_response(score, track_index, &profile, spec, seed)
        }
        RelationMode::OctaveDouble => {
            arrange_octave_double(score, track_index, &profile, spec, seed)
        }
        RelationMode::CounterMelody => {
            arrange_counter_melody(score, track_index, &profile, spec, seed)
        }
    }
}

/// `counter_melody`: an independent line against A, delegated to the S6
/// generator's `ConstrainedRandomWalk` — the one mode that synthesises a fresh
/// sequence instead of deriving B from A's grid.
///
/// The request is compiled from A: A's pitch classes as the scale, A's band
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
            intervals: scale_intervals_from(profile),
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
fn arrange_rhythm_lock(
    score: &Score,
    track_index: usize,
    profile: &PartProfile,
    spec: ComplementSpec,
    seed: GenerationSeed,
) -> Result<ComplementCandidate, ComplementError> {
    // Register band for B: A's band shifted, clamped, and ordered.
    let register = profile.register.ok_or(ComplementError::PartHasNoNotes)?;
    let (band_lo, band_hi) = shifted_band(register, spec.register_offset);

    let a_track = score
        .tracks
        .get(track_index)
        .ok_or(ComplementError::TrackIndexOutOfRange)?;
    let event_groups = grid_locked_groups(a_track, profile, band_lo, band_hi, seed);

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
fn arrange_register_contrast(
    score: &Score,
    track_index: usize,
    profile: &PartProfile,
    spec: ComplementSpec,
    seed: GenerationSeed,
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
    let event_groups = grid_locked_groups(a_track, profile, band_lo, band_hi, seed);

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
/// preceding A note, pitched seed-deterministically from A's pitch classes in
/// the band shifted by `spec.register_offset`. B's onsets are disjoint from
/// A's by construction. No qualifying gap is
/// [`ComplementError::NoGapsToAnswer`].
fn arrange_call_response(
    score: &Score,
    track_index: usize,
    profile: &PartProfile,
    spec: ComplementSpec,
    seed: GenerationSeed,
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
    let scale = scale_intervals_from(profile);
    let event_groups: Vec<EventGroup> = gaps
        .iter()
        .filter(|(start, end)| end.saturating_sub(*start) >= min_gap)
        .enumerate()
        .map(|(i, &(start, end))| {
            let degree = pitch_index(seed.0, i, scale.len());
            let pitch_val = degree_to_pitch(band_lo, band_hi, &scale, degree);
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
/// from A's pitch classes mapped into the `[band_lo, band_hi]` register band.
fn grid_locked_groups(
    a_track: &Track,
    profile: &PartProfile,
    band_lo: u8,
    band_hi: u8,
    seed: GenerationSeed,
) -> Vec<EventGroup> {
    // Scale: A's distinct pitch classes, as intervals above the band's low note.
    let intervals = scale_intervals_from(profile);

    voice_notes(a_track)
        .iter()
        .enumerate()
        .map(|(i, n)| {
            let degree = pitch_index(seed.0, i, intervals.len());
            let pitch_val = degree_to_pitch(band_lo, band_hi, &intervals, degree);
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

/// A's distinct pitch classes (semitone offsets from A's lowest pitch), sorted
/// and non-empty — the scale B substitutes pitches from.
fn scale_intervals_from(profile: &PartProfile) -> Vec<u8> {
    let min_a = profile.pitches.first().copied().unwrap_or(0);
    let mut intervals: Vec<u8> = profile
        .pitches
        .iter()
        .map(|&p| p.saturating_sub(min_a).checked_rem(12).unwrap_or(0))
        .collect();
    intervals.sort_unstable();
    intervals.dedup();
    if intervals.is_empty() {
        intervals.push(0);
    }
    intervals
}

/// Maps a scale `degree` to a concrete pitch inside `[lo, hi]`.
fn degree_to_pitch(lo: u8, hi: u8, intervals: &[u8], degree: usize) -> u8 {
    let idx = degree.checked_rem(intervals.len()).unwrap_or(0);
    let interval = intervals.get(idx).copied().unwrap_or(0);
    let raw = u16::from(lo).saturating_add(u16::from(interval));
    let clamped = raw.clamp(u16::from(lo), u16::from(hi));
    u8::try_from(clamped).unwrap_or(hi)
}

/// Seed-deterministic scale-degree picker for note `index` (`SplitMix64` finalizer).
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
