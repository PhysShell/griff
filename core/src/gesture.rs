//! Burst/rest gesture statistics — the DGD burst-and-rest case
//! (`docs/audit/2026-06-melodic-closure-research.md` §3.5, §7.4).
//!
//! What the corpus should teach for burst writing is **distributions, not
//! content**: how long the flurries are, where the rests sit on the metrical
//! grid, what the bursts land on, and how much the landing lengthens.
//! [`measure_gesture`] derives those distributions from a score track as
//! compact [`GestureStats`] — serialisable numbers that join the persisted
//! chunk axes (corpus schema v3) and later become S6 constraint inputs.
//!
//! Conventions (shared with the closure scorer, the novelty guard, and
//! `griff curate`): the analysed line is the highest-pitch-per-onset melody
//! of the track's first voice — a chord contributes its top note with that
//! note's duration. Segmentation:
//!
//! - a **gesture rest** is at least one quarter (`score.ticks_per_quarter`)
//!   of line silence *after a sounded note* — the trailing gap to the span
//!   end included, leading silence excluded (a rest belongs to the gesture
//!   before it; research note §1.3: a rest reads as part of the gesture when
//!   it is metrically predictable, as a hole when it is not);
//! - a **burst** is a maximal run of line notes between gesture rests;
//!   sub-quarter holes are phrasing, not rests.
//!
//! Derived shares use closure-v1 conventions where one exists:
//! burst-final lengthening is `landing / (2 × mean)` clamped to `[0, 1]`
//! (equal-to-mean reads 0.5), and the landing degree is measured against the
//! line's *modal* pitch class (ties to the smallest class) — a key-free root
//! proxy until `PartProfile` grows real harmonic context. Pure and
//! deterministic (SPEC §6); no RNG, no graph/DP dependency.

use std::mem;

use serde::{Deserialize, Serialize};

use crate::score::{AtomEvent, Score, Track};

/// Burst/rest gesture distributions of one track's melodic line.
///
/// Serialisable since corpus schema v3, where it persists into `ChunkMeta`
/// records next to `StructureMetrics`. Shares are in `[0, 1]`; lengths are in
/// line notes (bursts) or quarters (rests).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GestureStats {
    /// Line notes analysed (one per onset; a chord is its top note).
    pub note_count: usize,
    /// Maximal note runs between gesture rests (≥ 1 when measured).
    pub burst_count: usize,
    /// Mean burst length, in line notes.
    pub mean_burst_notes: f64,
    /// Longest burst, in line notes.
    pub max_burst_notes: usize,
    /// Gesture rests: line silences of at least one quarter after a sounded
    /// note, the trailing gap to the span end included.
    pub rest_count: usize,
    /// Mean gesture-rest length, in quarters (`0.0` when there are no rests).
    pub mean_rest_quarters: f64,
    /// Share of gesture rests starting on the quarter grid of their bar
    /// (metrical predictability); `1.0` when there are no rests (vacuous).
    pub rest_on_grid_share: f64,
    /// Share of bursts landing on the line's modal pitch class.
    pub modal_landing_share: f64,
    /// Mean burst-final lengthening: per burst, landing duration against
    /// twice the mean note duration, clamped to `[0, 1]` (closure-v1:
    /// equal-to-mean reads 0.5).
    pub mean_final_lengthening: f64,
}

/// Errors [`measure_gesture`] can emit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GestureError {
    /// The score has no master bars to analyse.
    EmptyScore,
    /// The requested track index is out of range.
    TrackIndexOutOfRange,
    /// The track's first voice contains no notes.
    NoNotes,
}

/// One melodic-line entry: `(onset, duration, pitch)`.
type LineNote = (u32, u32, u8);

/// Measures the burst/rest gesture distributions of the track at
/// `track_index`. Deterministic for fixed inputs.
pub fn measure_gesture(score: &Score, track_index: usize) -> Result<GestureStats, GestureError> {
    if score.master_bars.is_empty() {
        return Err(GestureError::EmptyScore);
    }
    let track = score
        .tracks
        .get(track_index)
        .ok_or(GestureError::TrackIndexOutOfRange)?;
    let line = top_line(track);
    if line.is_empty() {
        return Err(GestureError::NoNotes);
    }

    let quarter = u32::from(score.ticks_per_quarter.max(1));
    let span_end = score.master_bars.last().map_or(0, |mb| mb.tick_range.end.0);

    // Segment the line into bursts; collect the gesture rests after them.
    let mut bursts: Vec<Vec<LineNote>> = Vec::new();
    let mut current: Vec<LineNote> = Vec::new();
    let mut rests: Vec<(u32, u32)> = Vec::new(); // (start, length)
    for (i, &note) in line.iter().enumerate() {
        let (onset, duration, _) = note;
        let end = onset.saturating_add(duration);
        let next_onset = line.get(i.saturating_add(1)).map(|&(o, _, _)| o);
        current.push(note);
        let gap_end = next_onset.unwrap_or(span_end);
        let gap = gap_end.saturating_sub(end);
        if gap >= quarter {
            rests.push((end, gap));
            bursts.push(mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        bursts.push(current);
    }

    let modal = modal_pitch_class(&line);
    let landings_on_modal = bursts
        .iter()
        .filter(|b| {
            b.last()
                .is_some_and(|&(_, _, p)| p.checked_rem(12) == Some(modal))
        })
        .count();
    let total_burst_notes: usize = bursts.iter().map(Vec::len).sum();
    let max_burst_notes = bursts.iter().map(Vec::len).max().unwrap_or(0);
    let lengthening_sum: f64 = bursts.iter().map(|b| final_lengthening(b)).sum();

    let on_grid = rests
        .iter()
        .filter(|&&(start, _)| on_quarter_grid(start, quarter, score))
        .count();
    let rest_ticks: u64 = rests.iter().map(|&(_, len)| u64::from(len)).sum();

    Ok(GestureStats {
        note_count: line.len(),
        burst_count: bursts.len(),
        mean_burst_notes: ratio(total_burst_notes, bursts.len()),
        max_burst_notes,
        rest_count: rests.len(),
        mean_rest_quarters: rest_quarters_mean(rest_ticks, rests.len(), quarter),
        rest_on_grid_share: share_or_vacuous(on_grid, rests.len()),
        modal_landing_share: ratio(landings_on_modal, bursts.len()),
        mean_final_lengthening: lengthening_sum / denominator(bursts.len()),
    })
}

/// The melodic line of the track's first voice as `(onset, duration, pitch)`,
/// one entry per onset in onset order; a chord folds to its highest pitch,
/// carrying that note's duration (the closure / novelty convention).
fn top_line(track: &Track) -> Vec<LineNote> {
    let Some(voice) = track.voices.first() else {
        return Vec::new();
    };
    let mut notes: Vec<LineNote> = voice
        .event_groups
        .iter()
        .flat_map(|g| &g.atoms)
        .filter_map(|a| match a {
            AtomEvent::Note(n) => Some((n.absolute_start.0, n.duration.0, n.pitch.0)),
            AtomEvent::Rest(_) => None,
        })
        .collect();
    notes.sort_unstable_by_key(|&(onset, _, pitch)| (onset, pitch));

    let mut line: Vec<LineNote> = Vec::with_capacity(notes.len());
    for (onset, duration, pitch) in notes {
        match line.last_mut() {
            // Sorted by (onset, pitch): the last same-onset entry always
            // carries the highest pitch seen so far.
            Some((last_onset, last_duration, last_pitch)) if *last_onset == onset => {
                if pitch >= *last_pitch {
                    *last_pitch = pitch;
                    *last_duration = duration;
                }
            }
            _ => line.push((onset, duration, pitch)),
        }
    }
    line
}

/// The line's most frequent pitch class; ties go to the smallest class.
fn modal_pitch_class(line: &[LineNote]) -> u8 {
    let mut counts = [0_usize; 12];
    for &(_, _, pitch) in line {
        let class = usize::from(pitch.checked_rem(12).unwrap_or(0));
        if let Some(slot) = counts.get_mut(class) {
            *slot = slot.saturating_add(1);
        }
    }
    let best = counts.iter().max().copied().unwrap_or(0);
    let class = counts.iter().position(|&c| c == best).unwrap_or(0);
    u8::try_from(class).unwrap_or(0)
}

/// Burst-final lengthening under the closure-v1 normalisation: the landing
/// note's duration against twice the burst's mean duration, clamped to
/// `[0, 1]` — equal-to-mean reads 0.5.
#[allow(clippy::cast_precision_loss)] // note counts/durations are tiny vs f64 mantissa
fn final_lengthening(burst: &[LineNote]) -> f64 {
    let total: u64 = burst.iter().map(|&(_, d, _)| u64::from(d)).sum();
    let mean = total as f64 / denominator(burst.len());
    if mean <= 0.0 {
        return 0.5;
    }
    let landing = burst.last().map_or(0, |&(_, d, _)| d);
    (f64::from(landing) / (2.0 * mean)).clamp(0.0, 1.0)
}

/// Whether `tick` falls on the quarter grid of the master bar containing it
/// (or, before/after all bars, of the absolute grid).
fn on_quarter_grid(tick: u32, quarter: u32, score: &Score) -> bool {
    let bar_start = score
        .master_bars
        .iter()
        .rev()
        .find(|mb| mb.tick_range.start.0 <= tick)
        .map_or(0, |mb| mb.tick_range.start.0);
    tick.saturating_sub(bar_start)
        .checked_rem(quarter.max(1))
        .unwrap_or(0)
        == 0
}

/// `numerator / denominator` as f64; `0.0` over an empty denominator.
#[allow(clippy::cast_precision_loss)] // counts are tiny vs f64 mantissa
fn ratio(numerator: usize, denom: usize) -> f64 {
    if denom == 0 {
        0.0
    } else {
        numerator as f64 / denom as f64
    }
}

/// `taken / total`, or `1.0` when there is nothing to take (vacuously
/// predictable — the absence of rests is not unpredictability).
#[allow(clippy::cast_precision_loss)] // counts are tiny vs f64 mantissa
fn share_or_vacuous(taken: usize, total: usize) -> f64 {
    if total == 0 {
        1.0
    } else {
        taken as f64 / total as f64
    }
}

/// Mean rest length in quarters; `0.0` when there are no rests.
#[allow(clippy::cast_precision_loss)] // tick sums are tiny vs f64 mantissa
fn rest_quarters_mean(rest_ticks: u64, rest_count: usize, quarter: u32) -> f64 {
    if rest_count == 0 {
        return 0.0;
    }
    (rest_ticks as f64 / denominator(rest_count)) / f64::from(quarter.max(1))
}

/// A count as a safe f64 divisor (`1.0` floor).
#[allow(clippy::cast_precision_loss)] // counts are tiny vs f64 mantissa
fn denominator(count: usize) -> f64 {
    (count.max(1)) as f64
}
