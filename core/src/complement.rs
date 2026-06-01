//! `ComplementArranger` — rule-based complementary-part generation (S13).
//!
//! Given an existing part A on a [`Score`], `ComplementArranger` analyses A into a
//! [`PartProfile`], picks a [`RelationMode`], *derives* a concrete S6 generation
//! request for part B, delegates to [`crate::generate::generate`], and appends B
//! as a new [`Track`] on A's shared [`crate::score::MasterBar`] timeline
//! (ADR-0003). It is a constraint compiler over the S6 generator, not a new
//! generator (ADR-0012).
//!
//! This module is the first vertical slice: the `rhythm_lock` mode plus a
//! minimal pair validator. The other relation modes are named but not yet
//! implemented; they return [`ComplementError::ModeNotImplemented`].

use std::collections::BTreeSet;

use crate::event::{Articulation, Pitch, Ticks};
use crate::feature::PitchRange;
use crate::generate::{
    generate, GenerationConstraints, GenerationError, GenerationSeed, GenerationStrategy,
    PitchMaterial, RuleGenerationRequest,
};
use crate::score::{AtomEvent, Score, Track, Voice};

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

/// `rhythm_lock`: reuse the S6 `RhythmCopyPitchSubstitute` strategy with A's
/// onset grid as `source_rhythms` and pitch material derived from A's harmony,
/// shifted into the register requested by `spec.register_offset`.
fn arrange_rhythm_lock(
    score: &Score,
    track_index: usize,
    profile: &PartProfile,
    spec: ComplementSpec,
    seed: GenerationSeed,
) -> Result<ComplementCandidate, ComplementError> {
    // master_bars is non-empty (checked by the caller).
    let first_bar = score
        .master_bars
        .first()
        .ok_or(ComplementError::EmptyScore)?;

    // Register band for B: A's band shifted, clamped, and ordered.
    let register = profile.register.ok_or(ComplementError::PartHasNoNotes)?;
    let lo_shift = shift_pitch(register.lowest.0, spec.register_offset);
    let hi_shift = shift_pitch(register.highest.0, spec.register_offset);
    let pitch_lo = Pitch::new(lo_shift.min(hi_shift)).unwrap_or(register.lowest);
    let pitch_hi = Pitch::new(lo_shift.max(hi_shift)).unwrap_or(register.highest);

    // Pitch material: A's pitch classes as intervals above the band's low note.
    let pitch_material = pitch_material_from(profile, pitch_lo);

    // Rhythm template: the first non-empty bar of A's onset grid.
    let template = profile
        .bar_rhythms
        .iter()
        .find(|r| !r.is_empty())
        .cloned()
        .ok_or(ComplementError::PartHasNoNotes)?;

    let constraints = GenerationConstraints {
        bar_count: score.master_bars.len(),
        time_signature: first_bar.time_signature,
        tempo: first_bar.tempo,
        ticks_per_quarter: Ticks(u32::from(score.ticks_per_quarter)),
        pitch_lo,
        pitch_hi,
    };

    let request = RuleGenerationRequest {
        seed,
        pitch_material,
        constraints,
        source_rhythms: vec![template],
        strategy: GenerationStrategy::RhythmCopyPitchSubstitute,
    };

    let generated = generate(&request).map_err(ComplementError::Generation)?;

    // Lift B's single voice onto A's score as a new track on the shared bars.
    let b_voice = generated
        .score
        .tracks
        .into_iter()
        .next()
        .and_then(|t| t.voices.into_iter().next())
        .unwrap_or(Voice {
            id: 0,
            event_groups: Vec::new(),
        });

    let a_channel = score.tracks.get(track_index).map_or(0, |t| t.channel);
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

    let axis_scores = score_axes(profile, &combined, part_b_index, pitch_lo, pitch_hi);

    Ok(ComplementCandidate {
        score: combined,
        part_b_index,
        mode: spec.mode,
        seed,
        axis_scores,
    })
}

/// Builds pitch material from A: root at the band low, intervals = A's distinct
/// pitch classes measured from A's lowest pitch.
fn pitch_material_from(profile: &PartProfile, root: Pitch) -> PitchMaterial {
    let min_a = profile.pitches.first().copied().unwrap_or(root.0);
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
    PitchMaterial { root, intervals }
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
