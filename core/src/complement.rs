//! `ComplementArranger` — rule-based complementary-part generation (S13).
//!
//! Given an existing part A on a [`Score`], `ComplementArranger` analyses A into a
//! [`PartProfile`], picks a [`RelationMode`], *derives* part B, and appends it as
//! a new [`Track`] on A's shared [`crate::score::MasterBar`] timeline (ADR-0003).
//! It compiles a relative intent into the canonical model rather than being a new
//! generation core (ADR-0012); modes that synthesise fresh pitch sequences will
//! delegate to the S6 generator ([`crate::generate::generate`]).
//!
//! This module is the first vertical slice: the `rhythm_lock` mode plus a
//! minimal pair validator. `rhythm_lock` places B on A's exact onset grid, so it
//! reuses A's onsets directly rather than the S6 generator. The other relation
//! modes are named but not yet implemented; they return
//! [`ComplementError::ModeNotImplemented`].

use std::collections::BTreeSet;

use crate::event::{Articulation, Pitch, Ticks, Velocity};
use crate::feature::PitchRange;
use crate::generate::{GenerationError, GenerationSeed};
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
    /// Distinct articulations present across the part.
    pub techniques: BTreeSet<Articulation>,
}

/// Relation-as-provenance: per-axis scores describing how B relates to A.
#[derive(Debug, Clone, Copy)]
pub struct AxisScores {
    /// `1.0` when B's rhythm is identical to A's (`rhythm_lock`), else lower.
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
}

impl PairValidation {
    /// A pair is clean when there are no coincident dissonances and no register mud.
    #[must_use]
    pub const fn is_clean(&self) -> bool {
        self.coincident_dissonances == 0 && !self.register_mud
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
    /// The relation mode is named but not yet implemented in this slice.
    ModeNotImplemented(RelationMode),
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
    articulation: Option<Articulation>,
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
                articulation: n.articulation,
            }),
            AtomEvent::Rest(_) => None,
        })
        .collect();
    notes.sort_by_key(|n| n.onset);
    notes
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

    let techniques: BTreeSet<Articulation> = notes.iter().filter_map(|n| n.articulation).collect();

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
        other => Err(ComplementError::ModeNotImplemented(other)),
    }
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
    let band_lo = shift_pitch(register.lowest.0, spec.register_offset);
    let band_hi = shift_pitch(register.highest.0, spec.register_offset);
    let band_lo = band_lo.min(band_hi);
    let band_hi = band_lo.max(band_hi);

    // Scale: A's distinct pitch classes, as intervals above the band's low note.
    let intervals = scale_intervals_from(profile);

    let a_track = score
        .tracks
        .get(track_index)
        .ok_or(ComplementError::TrackIndexOutOfRange)?;
    let a_notes = voice_notes(a_track);

    let event_groups: Vec<EventGroup> = a_notes
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
                    articulation: None,
                })],
                technique_spans: Vec::new(),
            }
        })
        .collect();

    let b_voice = Voice {
        id: 0,
        event_groups,
    };

    let a_channel = a_track.channel;
    let b_channel = if a_channel >= 15 {
        0
    } else {
        a_channel.saturating_add(1)
    };

    let b_track = Track {
        name: Some(format!("Complement ({})", spec.mode.label())),
        channel: b_channel,
        voices: vec![b_voice],
    };

    let mut combined = score.clone();
    combined.tracks.push(b_track);
    let part_b_index = combined.tracks.len().saturating_sub(1);

    let band_lo_pitch = Pitch::new(band_lo).unwrap_or(register.lowest);
    let band_hi_pitch = Pitch::new(band_hi).unwrap_or(register.highest);
    let axis_scores = score_axes(
        profile,
        &combined,
        part_b_index,
        band_lo_pitch,
        band_hi_pitch,
    );

    Ok(ComplementCandidate {
        score: combined,
        part_b_index,
        mode: spec.mode,
        seed,
        axis_scores,
    })
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
fn score_axes(
    profile: &PartProfile,
    combined: &Score,
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

    let b_techniques: BTreeSet<Articulation> =
        b_notes.iter().filter_map(|n| n.articulation).collect();
    let technique_overlap = jaccard(&profile.techniques, &b_techniques);

    AxisScores {
        // rhythm_lock reproduces A's onset grid exactly.
        rhythm_similarity: 1.0,
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

/// Jaccard overlap of two technique sets; `1.0` when both are empty.
#[allow(clippy::cast_precision_loss)]
fn jaccard(a: &BTreeSet<Articulation>, b: &BTreeSet<Articulation>) -> f64 {
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

/// Validates the (A, B) pair: counts dissonances on coincident onsets and flags
/// register mud (the two parts overlapping the same register too heavily).
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
    })
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
