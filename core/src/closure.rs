//! Closure axis v1 — does a phrase feel *finished*? (ADR-0017;
//! `docs/audit/2026-06-melodic-closure-research.md` §3.1–§3.3, §7.2).
//!
//! "Closure" is the music-cognition term for the perceptual sense that a
//! melody has ended rather than broken off — *melodic* closure, not a Rust
//! closure. Four named axes, each in `[0, 1]`, measured over the first voice
//! of a score track (consistent with the S4 boundary detector, which this
//! module re-uses as the generation-side referee):
//!
//! - `internal_continuity` — `1 −` the strongest phrase boundary the S4
//!   detector finds strictly inside the phrase. A spurious mid-phrase break
//!   is the "three notes and awkward rests" failure mode.
//! - `ending_stability` — the landing note's scale-degree stability relative
//!   to the [`PitchMaterial`], on a simplified Krumhansl-inspired tier table.
//! - `final_lengthening` — phrase-final lengthening: the landing note's
//!   duration against twice the mean note duration.
//! - `gap_fill` — Narmour implication-realization mini-rules over the final
//!   intervals of the highest-pitch-per-onset melodic line.
//!
//! The track is treated as a single phrase; scoring multi-phrase spans with
//! intended internal seams (S14 tiles) is a later increment. Pure and
//! deterministic (SPEC §6): no RNG, so [`Scored`](crate::scoring::Scored)
//! envelopes built from these axes carry `seed: None`.

use crate::{
    boundary::{detect_phrase_boundaries, BoundaryConfig},
    event::Ticks,
    generate::PitchMaterial,
    score::{AtomEvent, Score, Track},
    scoring::{Axes, Axis, WeightPolicy},
};

const AXIS_INTERNAL_CONTINUITY: &str = "internal_continuity";
const AXIS_ENDING_STABILITY: &str = "ending_stability";
const AXIS_FINAL_LENGTHENING: &str = "final_lengthening";
const AXIS_GAP_FILL: &str = "gap_fill";

/// The closure axes, in their canonical order (ADR-0017).
pub const CLOSURE_AXIS_LABELS: [&str; 4] = [
    AXIS_INTERNAL_CONTINUITY,
    AXIS_ENDING_STABILITY,
    AXIS_FINAL_LENGTHENING,
    AXIS_GAP_FILL,
];

/// Errors the closure scorer can emit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClosureError {
    /// The score has no master bars.
    EmptyScore,
    /// The track index is out of range.
    TrackIndexOutOfRange,
    /// The track's first voice contains no notes.
    NoNotes,
}

/// A note as `(onset, duration, pitch)`, kept in onset order.
type NoteTuple = (u32, u32, u8);

/// Measures the four closure axes of the first voice of `track_index`,
/// treating the whole track as a single phrase.
///
/// Deterministic for a fixed input: rerank candidates with the axes under
/// [`closure_weights_v1`] via [`Scored`](crate::scoring::Scored) and
/// [`rank_indices`](crate::scoring::rank_indices).
pub fn closure_axes(
    score: &Score,
    track_index: usize,
    material: &PitchMaterial,
) -> Result<Axes, ClosureError> {
    if score.master_bars.is_empty() {
        return Err(ClosureError::EmptyScore);
    }
    let track = score
        .tracks
        .get(track_index)
        .ok_or(ClosureError::TrackIndexOutOfRange)?;
    let notes = collect_notes(track);
    if notes.is_empty() {
        return Err(ClosureError::NoNotes);
    }

    Ok(Axes::new(vec![
        Axis {
            label: AXIS_INTERNAL_CONTINUITY,
            value: internal_continuity(score, track_index, &notes),
        },
        Axis {
            label: AXIS_ENDING_STABILITY,
            value: ending_stability(&notes, material),
        },
        Axis {
            label: AXIS_FINAL_LENGTHENING,
            value: final_lengthening(&notes),
        },
        Axis {
            label: AXIS_GAP_FILL,
            value: gap_fill(&melodic_line(&notes)),
        },
    ]))
}

/// The baseline closure weight policy (`closure` v1): uniform over the four
/// axes.
///
/// Untuned by design — weights are *data* the feedback layer (S9) learns
/// (ADR-0017 §3), and the swancore-specific weighting awaits S5 corpus
/// calibration (research note §6).
#[must_use]
pub fn closure_weights_v1() -> WeightPolicy {
    WeightPolicy::uniform("closure", 1, &CLOSURE_AXIS_LABELS)
}

// ── note collection ───────────────────────────────────────────────────────────

/// All notes of the track's first voice as `(onset, duration, pitch)`, sorted
/// by onset then pitch (the same stream the S4 referee reads).
fn collect_notes(track: &Track) -> Vec<NoteTuple> {
    let Some(voice) = track.voices.first() else {
        return Vec::new();
    };
    let mut notes: Vec<NoteTuple> = voice
        .event_groups
        .iter()
        .flat_map(|g| &g.atoms)
        .filter_map(|a| match a {
            AtomEvent::Note(n) => Some((n.absolute_start.0, n.duration.0, n.pitch.0)),
            AtomEvent::Rest(_) => None,
        })
        .collect();
    notes.sort_unstable_by_key(|&(onset, _, pitch)| (onset, pitch));
    notes
}

// ── internal continuity (the S4 referee) ──────────────────────────────────────

/// `1 −` the strongest boundary the S4 detector places strictly inside the
/// phrase span `(first onset, last note end)`; `1.0` when none fires.
///
/// The referee runs the default S4 weights and threshold with the tick
/// parameters derived from the score's own resolution (the S4 defaults assume
/// PPQN 960): snap grid 1/16, minimum boundary gap two quarter notes.
fn internal_continuity(score: &Score, track_index: usize, notes: &[NoteTuple]) -> f64 {
    let ppqn = u32::from(score.ticks_per_quarter);
    let config = BoundaryConfig {
        // Reason: ppqn / 4 and ppqn * 2 on a u32 PPQN cannot overflow or
        // divide by zero.
        #[allow(clippy::arithmetic_side_effects)]
        quantize_ticks: Ticks(ppqn / 4),
        min_gap: Ticks(ppqn.saturating_mul(2)),
        ..BoundaryConfig::default()
    };

    let first_onset = notes.first().map_or(0, |&(onset, _, _)| onset);
    let last_end = notes
        .iter()
        .map(|&(onset, duration, _)| onset.saturating_add(duration))
        .max()
        .unwrap_or(0);

    let strongest = detect_phrase_boundaries(score, track_index, &config)
        .iter()
        .filter(|b| b.start_tick.0 > first_onset && b.start_tick.0 < last_end)
        .map(|b| b.score)
        .fold(0.0_f64, f64::max);

    1.0 - strongest.clamp(0.0, 1.0)
}

// ── ending stability (Krumhansl-inspired tiers) ───────────────────────────────

/// Scale-degree stability of the landing onset, the most stable note winning
/// when the phrase ends on a chord.
///
/// Tier table (v1, simplified from the Krumhansl–Kessler probe-tone shape):
/// root `1.0`; in-material perfect fifth `0.8`; in-material third `0.7`; any
/// other in-material degree `0.5`; outside the material `0.2`.
// Reason: pitch arithmetic on i16 (values ≤ 127) cannot overflow; modulo is
// by the constant 12.
#[allow(clippy::arithmetic_side_effects)]
fn ending_stability(notes: &[NoteTuple], material: &PitchMaterial) -> f64 {
    let last_onset = notes.last().map_or(0, |&(onset, _, _)| onset);
    let in_material = |offset: i16| {
        material
            .intervals
            .iter()
            .any(|&i| i16::from(i % 12) == offset)
    };

    notes
        .iter()
        .filter(|&&(onset, _, _)| onset == last_onset)
        .map(|&(_, _, pitch)| {
            let offset = (i16::from(pitch) - i16::from(material.root.0)).rem_euclid(12);
            if offset == 0 {
                1.0
            } else if offset == 7 && in_material(7) {
                0.8
            } else if (offset == 3 || offset == 4) && in_material(offset) {
                0.7
            } else if in_material(offset) {
                0.5
            } else {
                0.2
            }
        })
        .fold(0.0_f64, f64::max)
}

// ── final lengthening ─────────────────────────────────────────────────────────

/// The landing note's duration against twice the mean note duration, clamped
/// to `[0, 1]`: equal to the mean reads `0.5`, twice the mean or longer reads
/// `1.0`. On a landing chord the longest note counts.
fn final_lengthening(notes: &[NoteTuple]) -> f64 {
    let total: u64 = notes
        .iter()
        .map(|&(_, duration, _)| u64::from(duration))
        .sum();
    // Reason: the note count is tiny relative to f64 mantissa precision.
    #[allow(clippy::cast_precision_loss)]
    let count = notes.len() as f64;
    #[allow(clippy::cast_precision_loss)]
    let mean = total as f64 / count.max(1.0);
    if mean <= 0.0 {
        return 0.5;
    }

    let last_onset = notes.last().map_or(0, |&(onset, _, _)| onset);
    let landing = notes
        .iter()
        .filter(|&&(onset, _, _)| onset == last_onset)
        .map(|&(_, duration, _)| duration)
        .max()
        .unwrap_or(0);

    (f64::from(landing) / (2.0 * mean)).clamp(0.0, 1.0)
}

// ── gap fill (Narmour mini-rules) ─────────────────────────────────────────────

/// The melodic line as the highest pitch per onset, in onset order.
fn melodic_line(notes: &[NoteTuple]) -> Vec<u8> {
    let mut line: Vec<(u32, u8)> = Vec::new();
    for &(onset, _, pitch) in notes {
        match line.last_mut() {
            Some((last_onset, last_pitch)) if *last_onset == onset => {
                *last_pitch = (*last_pitch).max(pitch);
            }
            _ => line.push((onset, pitch)),
        }
    }
    line.into_iter().map(|(_, pitch)| pitch).collect()
}

/// Narmour implication-realization mini-rules over the line's final
/// intervals, tiered (rules checked in order):
///
/// - final interval larger than a perfect fifth — an unresolved leap: `0.0`;
/// - a leap (≥ 5 semitones) answered by a smaller interval in the opposite
///   direction — the gap is filled: `1.0`;
/// - stepwise approach (≤ 2 semitones): `0.8`;
/// - anything else (a medium 3–7 semitone approach): `0.4`;
/// - fewer than two line notes: `0.5` (neutral — no interval to judge).
// Reason: intervals between MIDI pitches (≤ 127) fit i16 with a wide margin.
#[allow(clippy::arithmetic_side_effects)]
fn gap_fill(line: &[u8]) -> f64 {
    let interval = |from: u8, to: u8| i16::from(to) - i16::from(from);
    let (implicative, realized) = match *line {
        [] | [_] => return 0.5,
        [.., a, b, c] => (Some(interval(a, b)), interval(b, c)),
        [a, b] => (None, interval(a, b)),
    };

    if realized.abs() > 7 {
        return 0.0;
    }
    if let Some(implicative) = implicative {
        let reversed = (implicative > 0) != (realized > 0);
        if implicative.abs() >= 5 && realized != 0 && reversed && realized.abs() < implicative.abs()
        {
            return 1.0;
        }
    }
    if realized.abs() <= 2 {
        0.8
    } else {
        0.4
    }
}
