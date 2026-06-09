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
//! Bar similarity is contour-aware: two bars are compared by their onset grid
//! (rhythm) and their pitches, and a *transposed* repeat — identical rhythm,
//! constant non-zero interval shift (`A A' A''`) — earns partial pitch credit.
//! So identical tiles read as high repeatability, transposed tiles as medium,
//! rhythm-only tiles as partial, and unrelated material as low. (This closes
//! the Phase-0 "transposed repeats" limitation; `structural_complexity` still
//! counts *exact* distinct signatures on purpose — a transposed sequence is
//! genuinely more material than a verbatim loop.)
//!
//! Known limitations (Phase 0, deferred refinements — ADR-0015):
//! - Metrics are computed over the score's master bars as given. A trailing
//!   empty bar lowers `loopability_score` (it reads as a whole-bar seam gap) and
//!   dilutes the period/repeatability scores. Note the MIDI importer appends one
//!   such sentinel bar when content ends exactly on a barline (see
//!   `midi::build_master_bars`); fixing that at the source is a follow-up.

use std::collections::BTreeSet;

use crate::score::{AtomEvent, Score, Track};

/// Minimum mean self-similarity at a lag for it to count as a pattern period.
const PERIOD_THRESHOLD: f64 = 0.5;

/// Weight of the rhythm (onset-grid) component in [`bar_similarity`]; the
/// remainder weights the pitch component.
const RHYTHM_WEIGHT: f64 = 0.5;

/// Pitch-component credit for a transposed repeat — identical rhythm, constant
/// non-zero interval shift. Half of an exact pitch match, so transposed tiles
/// land between verbatim tiles and rhythm-only tiles.
const TRANSPOSITION_CREDIT: f64 = 0.5;

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
                sum += bar_similarity(a, b);
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

/// Contour-aware similarity of two bar signatures, in `[0, 1]`.
///
/// A weighted sum of a rhythm component (Jaccard over onset sets) and a pitch
/// component (Jaccard over `(onset, pitch)` pairs). A *transposed* repeat —
/// identical rhythm with every pitch shifted by one constant non-zero interval
/// — lifts the pitch component to [`TRANSPOSITION_CREDIT`], so `A A' A''`
/// sequences read as medium similarity instead of none.
///
/// Two empty signatures (whole-bar rests) are treated as identical (`1.0`), so a
/// rest bar that is part of a repeated phrase still counts toward the period; an
/// empty bar against a non-empty one is fully dissimilar (`0.0`).
fn bar_similarity(a: &[(u32, u8)], b: &[(u32, u8)]) -> f64 {
    if a.is_empty() && b.is_empty() {
        return 1.0;
    }
    let rhythm = set_jaccard(
        &a.iter().map(|&(o, _)| o).collect(),
        &b.iter().map(|&(o, _)| o).collect(),
    );
    let exact = set_jaccard(&a.iter().copied().collect(), &b.iter().copied().collect());
    let pitch = if transposition_interval(a, b).is_some() {
        exact.max(TRANSPOSITION_CREDIT)
    } else {
        exact
    };
    RHYTHM_WEIGHT.mul_add(rhythm, (1.0 - RHYTHM_WEIGHT) * pitch)
}

/// Jaccard similarity of two sets; two empty sets count as identical (`1.0`).
#[allow(clippy::cast_precision_loss)]
fn set_jaccard<T: Ord>(sa: &BTreeSet<T>, sb: &BTreeSet<T>) -> f64 {
    let inter = sa.intersection(sb).count();
    let union = sa.union(sb).count();
    if union == 0 {
        1.0
    } else {
        inter as f64 / union as f64
    }
}

/// The constant non-zero interval by which `b` transposes `a`, if any.
///
/// Requires the same note count and pairwise-identical onsets; signatures are
/// sorted by `(onset, pitch)`, so chord notes align positionally and a chord
/// transposes only if every voice shifts by the same interval. Returns `None`
/// for empty bars and for a zero shift (that is an exact repeat).
fn transposition_interval(a: &[(u32, u8)], b: &[(u32, u8)]) -> Option<i32> {
    if a.is_empty() || a.len() != b.len() {
        return None;
    }
    let mut delta: Option<i32> = None;
    for (&(oa, pa), &(ob, pb)) in a.iter().zip(b.iter()) {
        if oa != ob {
            return None;
        }
        let d = i32::from(pb).saturating_sub(i32::from(pa));
        match delta {
            None => delta = Some(d),
            Some(prev) if prev != d => return None,
            Some(_) => {}
        }
    }
    delta.filter(|&d| d != 0)
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
/// to the first in pitch, and how little silence sits at the seam (trailing gap
/// after the last note plus leading gap before the first note).
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

    // Timing seam: total silence across the wrap point — the trailing gap from
    // the last note's end to the span end, plus the leading gap from the span
    // start to the first note's onset (the loop waits through both).
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
    let span_start = first_bar.tick_range.start.0;
    let span_end = last_bar.tick_range.end.0;
    let last_end = last.onset.saturating_add(last.duration);
    let trailing_gap = span_end.saturating_sub(last_end);
    let leading_gap = first.onset.saturating_sub(span_start);
    let seam_gap = trailing_gap.saturating_add(leading_gap);
    let timing_seam = if bar_ticks == 0 {
        0.0
    } else {
        1.0 - f64::from(seam_gap.min(bar_ticks)) / f64::from(bar_ticks)
    };

    (pitch_seam + timing_seam) / 2.0
}
