//! Structure metrics — the S14 Phase-0 measurement pass (ADR-0015).
//!
//! [`measure_structure`] derives [`StructureMetrics`] from a track over a score:
//! the detected pattern period, how strongly the material repeats, how much it
//! varies, how cleanly it loops, and its structural complexity. It answers
//! "what *kind* of musical organisation is this span?" rather than "what notes
//! are in it".
//!
//! This is a self-contained analysis in the feature layer. It is a pure,
//! deterministic function (SPEC §6) and depends on **neither** the graph layer
//! **nor** DP/Viterbi (ADR-0013): the metrics are designed to *become* graph
//! node attributes and reranking signals later, not to consume them (ADR-0015).
//!
//! Granularity: this first pass detects the period at *bar* resolution via
//! self-similarity autocorrelation over per-bar signatures. Sub-bar (beat-level)
//! periods and the full per-axis complexity profile are later increments.
//!
//! Known limitations (Phase 0, deferred refinements — ADR-0015):
//! - Bar signatures compare `(onset, absolute pitch)`, so a motif transposed
//!   bar-to-bar (`A A' A''`) reads as *low* repeatability rather than *medium*.
//!   A later pass may compare rhythm + interval contour. The intended use is a
//!   user-tunable control, so an exact-pitch baseline is acceptable for now.
//! - Metrics are computed over the score's master bars as given. A trailing
//!   empty bar lowers `loopability_score` (it reads as a whole-bar seam gap) and
//!   dilutes the period/repeatability scores. Note the MIDI importer appends one
//!   such sentinel bar when content ends exactly on a barline (see
//!   `midi::build_master_bars`); fixing that at the source is a follow-up.

use std::collections::BTreeSet;

use crate::score::{AtomEvent, Score, Track};

/// Minimum mean self-similarity at a lag for it to count as a pattern period.
const PERIOD_THRESHOLD: f64 = 0.5;

/// The measured structural character of a span (S14, ADR-0015).
///
/// The provenance counterpart of a `StructureControl`: it describes what a
/// produced or imported span *is*, for reranking and (later) graph attributes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StructureMetrics {
    /// Number of master bars analysed.
    pub bar_count: usize,
    /// Length of the dominant repeating idea, in bars, or `None` if through-composed.
    pub detected_pattern_period_bars: Option<usize>,
    /// The same period in ticks (sum of the first `period` bars' spans), or `None`.
    pub detected_pattern_period_ticks: Option<u32>,
    /// Strength of the dominant period: mean self-similarity at the best lag, in `[0, 1]`.
    pub repeatability_score: f64,
    /// How much repeats deviate from identical (`1 - repeatability`), in `[0, 1]`.
    pub variation_score: f64,
    /// How seamlessly the span loops — pitch + timing continuity at the wrap seam, in `[0, 1]`.
    pub loopability_score: f64,
    /// Fraction of distinct bar signatures, in `[0, 1]`: low = looped, high = through-composed.
    pub structural_complexity: f64,
}

/// Errors [`measure_structure`] can emit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StructureError {
    /// The score has no master bars to analyse.
    EmptyScore,
    /// The requested track index is out of range.
    TrackIndexOutOfRange,
}

/// One note flattened to absolute position.
#[derive(Debug, Clone, Copy)]
struct NoteRef {
    onset: u32,
    duration: u32,
    pitch: u8,
}

/// Collects all of a track's notes — across every voice — sorted by onset, so a
/// track's structure is measured as a whole (some importers split one track into
/// several voices).
fn track_notes(track: &Track) -> Vec<NoteRef> {
    let mut notes: Vec<NoteRef> = track
        .voices
        .iter()
        .flat_map(|v| &v.event_groups)
        .flat_map(|g| &g.atoms)
        .filter_map(|a| match a {
            AtomEvent::Note(n) => Some(NoteRef {
                onset: n.absolute_start.0,
                duration: n.duration.0,
                pitch: n.pitch.0,
            }),
            AtomEvent::Rest(_) => None,
        })
        .collect();
    notes.sort_by_key(|n| n.onset);
    notes
}

/// Measures the structural metrics of the track at `track_index`.
pub fn measure_structure(
    score: &Score,
    track_index: usize,
) -> Result<StructureMetrics, StructureError> {
    if score.master_bars.is_empty() {
        return Err(StructureError::EmptyScore);
    }
    let track = score
        .tracks
        .get(track_index)
        .ok_or(StructureError::TrackIndexOutOfRange)?;

    let notes = track_notes(track);
    let bar_count = score.master_bars.len();

    // Per-bar signature: the set of (onset-within-bar, pitch) pairs.
    let signatures: Vec<Vec<(u32, u8)>> = score
        .master_bars
        .iter()
        .map(|mb| {
            let mut sig: Vec<(u32, u8)> = notes
                .iter()
                .filter(|n| n.onset >= mb.tick_range.start.0 && n.onset < mb.tick_range.end.0)
                .map(|n| (n.onset.saturating_sub(mb.tick_range.start.0), n.pitch))
                .collect();
            sig.sort_unstable();
            sig.dedup();
            sig
        })
        .collect();

    let (period_bars, repeatability) = detect_period(&signatures);
    let variation = (1.0 - repeatability).clamp(0.0, 1.0);

    let detected_pattern_period_ticks = period_bars.map(|p| {
        score
            .master_bars
            .iter()
            .take(p)
            .map(|mb| mb.tick_range.end.0.saturating_sub(mb.tick_range.start.0))
            .fold(0_u32, u32::saturating_add)
    });

    Ok(StructureMetrics {
        bar_count,
        detected_pattern_period_bars: period_bars,
        detected_pattern_period_ticks,
        repeatability_score: repeatability,
        variation_score: variation,
        loopability_score: loopability(&notes, score),
        structural_complexity: unique_ratio(&signatures),
    })
}

/// Finds the lag (in bars) of strongest self-similarity. Returns the period
/// (if its strength clears the threshold) and the strength at the best lag.
#[allow(clippy::cast_precision_loss)]
fn detect_period(signatures: &[Vec<(u32, u8)>]) -> (Option<usize>, f64) {
    let n = signatures.len();
    if n < 2 {
        return (None, 0.0);
    }
    let max_lag = n / 2;
    let mut best_lag = 0_usize;
    let mut best = 0.0_f64;
    for lag in 1..=max_lag {
        let pairs = n.saturating_sub(lag);
        if pairs == 0 {
            continue;
        }
        let mut sum = 0.0_f64;
        for (i, a) in signatures.iter().enumerate().take(pairs) {
            if let Some(b) = signatures.get(i.saturating_add(lag)) {
                sum += jaccard(a, b);
            }
        }
        let mean = sum / pairs as f64;
        if mean > best {
            best = mean;
            best_lag = lag;
        }
    }
    if best >= PERIOD_THRESHOLD && best_lag > 0 {
        (Some(best_lag), best)
    } else {
        (None, best)
    }
}

/// Jaccard similarity of two bar signatures (sets of `(onset, pitch)` pairs).
///
/// Two empty signatures (whole-bar rests) are treated as identical (`1.0`), so a
/// rest bar that is part of a repeated phrase still counts toward the period; an
/// empty bar against a non-empty one is fully dissimilar (`0.0`).
#[allow(clippy::cast_precision_loss)]
fn jaccard(a: &[(u32, u8)], b: &[(u32, u8)]) -> f64 {
    let sa: BTreeSet<(u32, u8)> = a.iter().copied().collect();
    let sb: BTreeSet<(u32, u8)> = b.iter().copied().collect();
    let inter = sa.intersection(&sb).count();
    let union = sa.union(&sb).count();
    if union == 0 {
        1.0
    } else {
        inter as f64 / union as f64
    }
}

/// Fraction of distinct bar signatures: low for looped, high for through-composed.
#[allow(clippy::cast_precision_loss)]
fn unique_ratio(signatures: &[Vec<(u32, u8)>]) -> f64 {
    let n = signatures.len();
    if n == 0 {
        return 0.0;
    }
    let distinct: BTreeSet<&Vec<(u32, u8)>> = signatures.iter().collect();
    distinct.len() as f64 / n as f64
}

/// Loopability: continuity of the wrap-around seam — how close the last note is
/// to the first in pitch, and how completely the material fills to the span end.
#[allow(clippy::cast_precision_loss)]
fn loopability(notes: &[NoteRef], score: &Score) -> f64 {
    let (Some(first), Some(last)) = (notes.first(), notes.last()) else {
        return 0.0;
    };
    let (Some(first_bar), Some(last_bar)) = (score.master_bars.first(), score.master_bars.last())
    else {
        return 0.0;
    };

    // Pitch seam: an octave-or-more jump scores 0, a unison scores 1.
    let interval = i32::from(last.pitch)
        .saturating_sub(i32::from(first.pitch))
        .abs();
    let pitch_seam = 1.0 - f64::from(interval.min(12)) / 12.0;

    // Timing seam: how close the final note's end is to the span's end.
    let bar_ticks = last_bar
        .tick_range
        .end
        .0
        .saturating_sub(first_bar.tick_range.start.0)
        .min(
            last_bar
                .tick_range
                .end
                .0
                .saturating_sub(last_bar.tick_range.start.0),
        );
    let span_end = last_bar.tick_range.end.0;
    let last_end = last.onset.saturating_add(last.duration);
    let gap = span_end.abs_diff(last_end);
    let timing_seam = if bar_ticks == 0 {
        0.0
    } else {
        1.0 - f64::from(gap.min(bar_ticks)) / f64::from(bar_ticks)
    };

    (pitch_seam + timing_seam) / 2.0
}
