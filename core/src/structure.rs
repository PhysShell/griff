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
//! Granularity: the main pass detects the period at *bar* resolution via
//! self-similarity autocorrelation over per-bar signatures. A sub-bar
//! refinement autocorrelates per-*beat* cell signatures on uniform timelines
//! and reports the strongest verbatim-tiling lag shorter than one bar
//! (`detected_subbar_period_ticks`); the full per-axis complexity profile is a
//! later increment.
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
//! Phase 1 adds the *control* side: [`StructureControl`] plus
//! [`generate_structured`] — a tile/vary **constraint compiler over the S6
//! generator** (the `ComplementArranger` pattern, ADR-0012/0015), not a new
//! generation core. It generates a `pattern_period`-length base via S6, tiles
//! it across the target span, varies copies by rhythm-preserving transposition
//! (exactly the operator the contour-aware metric reads as a medium repeat),
//! and returns the produced score with its measured [`StructureMetrics`] as
//! provenance. Deterministic for a fixed `(request, seed)` (SPEC §6).
//!
//! Known limitations (Phase 0/1, deferred refinements — ADR-0015):
//! - Metrics are computed over the score's master bars as given. A trailing
//!   empty bar lowers `loopability_score` (it reads as a whole-bar seam gap) and
//!   dilutes the period/repeatability scores. Note the MIDI importer appends one
//!   such sentinel bar when content ends exactly on a barline (see
//!   `midi::build_master_bars`); fixing that at the source is a follow-up.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::event::{Pitch, Ticks, Tuning};
use crate::generate::{
    bar_duration_ticks, generate, GenerationConstraints, GenerationError, GenerationSeed,
    GenerationStrategy, PitchMaterial, RuleGenerationRequest,
};
use crate::score::{
    AtomEvent, AtomNote, EventGroup, EventGroupKind, LossReport, MasterBar, Score, Track, Voice,
};
use crate::scoring::{rank_indices, Axes, Axis, Scored, WeightPolicy};
use crate::slice::TickRange;

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
/// Serialisable since S14 Phase 3, where it persists into corpus `ChunkMeta`
/// records (schema v2).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct StructureMetrics {
    /// Number of master bars analysed.
    pub bar_count: usize,
    /// Length of the dominant repeating idea, in bars, or `None` if through-composed.
    pub detected_pattern_period_bars: Option<usize>,
    /// The same period in ticks (sum of the first `period` bars' spans), or `None`.
    pub detected_pattern_period_ticks: Option<u32>,
    /// Sub-bar refinement: the strongest *verbatim*-tiling lag shorter than one
    /// bar, in ticks (a whole number of beats), or `None` when no sub-bar cell
    /// repeats, the track is silent, or the timeline is not uniform. Optional
    /// in the corpus schema since v5; pre-v5 records load as `None`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detected_subbar_period_ticks: Option<u32>,
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
        detected_subbar_period_ticks: detect_subbar_period(&notes, score),
        repeatability_score: repeatability,
        variation_score: variation,
        loopability_score: loopability(&notes, score),
        structural_complexity: unique_ratio(&signatures),
    })
}

/// Sub-bar (beat-level) period: the strongest verbatim-tiling lag shorter than
/// one bar, in ticks, found by autocorrelation over per-beat cell signatures.
///
/// Requires notes and a uniform timeline (one shared time signature and bar
/// span) whose bar divides evenly into `numerator` beat cells; otherwise it
/// abstains (`None`). Cells are compared by exact `(onset-within-cell, pitch)`
/// signatures: at one-onset granularity the bar-level rhythm floor and
/// transposition credit are degenerate (almost any two single-note cells would
/// "match"), so the sub-bar pass demands verbatim repeats — softening that is
/// a later increment. Ties resolve to the shortest lag (strict-greater scan).
#[allow(clippy::cast_precision_loss)]
fn detect_subbar_period(notes: &[NoteRef], score: &Score) -> Option<u32> {
    if notes.is_empty() {
        return None;
    }
    let first = score.master_bars.first()?;
    let bar_span = first
        .tick_range
        .end
        .0
        .saturating_sub(first.tick_range.start.0);
    let uniform = score.master_bars.iter().all(|mb| {
        mb.time_signature == first.time_signature
            && mb.tick_range.end.0.saturating_sub(mb.tick_range.start.0) == bar_span
    });
    if !uniform {
        return None;
    }
    let beats = u32::from(first.time_signature.numerator);
    if beats < 2 {
        return None;
    }
    let beat_ticks = bar_span.checked_div(beats)?;
    if beat_ticks == 0 || beat_ticks.checked_mul(beats) != Some(bar_span) {
        return None;
    }

    // Per-beat cell signatures across the whole span, in timeline order.
    let mut cells: Vec<BTreeSet<(u32, u8)>> = Vec::new();
    for mb in &score.master_bars {
        for b in 0..beats {
            let cell_start = mb
                .tick_range
                .start
                .0
                .saturating_add(b.saturating_mul(beat_ticks));
            let cell_end = cell_start.saturating_add(beat_ticks);
            cells.push(
                notes
                    .iter()
                    .filter(|n| n.onset >= cell_start && n.onset < cell_end)
                    .map(|n| (n.onset.saturating_sub(cell_start), n.pitch))
                    .collect(),
            );
        }
    }

    let mut best_lag = 0_u32;
    let mut best = 0.0_f64;
    for lag in 1..beats {
        let lag_cells = usize::try_from(lag).ok()?;
        let pairs = cells.len().saturating_sub(lag_cells);
        if pairs == 0 {
            continue;
        }
        let mut sum = 0.0_f64;
        for (i, a) in cells.iter().enumerate().take(pairs) {
            if let Some(b) = cells.get(i.saturating_add(lag_cells)) {
                sum += set_jaccard(a, b);
            }
        }
        let mean = sum / pairs as f64;
        if mean > best {
            best = mean;
            best_lag = lag;
        }
    }
    if best >= PERIOD_THRESHOLD && best_lag > 0 {
        best_lag.checked_mul(beat_ticks)
    } else {
        None
    }
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

// ── S14 Phase 1: structure controls — the tile/vary compiler over S6 ──────────

/// Per-copy transposition intervals (semitones) used by the variation operator.
///
/// Consecutive entries always differ, and so do entries two apart, so adjacent
/// (and lag-2) varied copies never coincide — the detected period stays at the
/// requested tile length instead of an accidental multiple.
const VARIATION_INTERVALS: [i32; 6] = [3, -3, 5, -5, 7, -7];

/// Salt for the per-copy verbatim-vs-varied decision.
const COPY_SALT: u64 = 0x5354_5255_4354_5552; // "STRUCTUR"
/// Salt for the per-bar mutation gate inside a varied copy.
const BAR_SALT: u64 = 0x434F_4E54_524F_4C31; // "CONTROL1"
/// Salt for the seed-derived offset into [`VARIATION_INTERVALS`].
const INTERVAL_SALT: u64 = 0x494E_5445_5256_414C; // "INTERVAL"

/// What the caller asks of the structure compiler (ADR-0015 §1).
///
/// The Phase-1 subset of the control: time organisation only. The target span
/// is carried by `GenerationConstraints::bar_count`; loopability targets and
/// the `ComplexityProfile` are later increments.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StructureControl {
    /// Length of the base repeating idea, in bars. `None` = through-composed:
    /// the request is delegated to plain S6 with no tiling.
    pub pattern_period_bars: Option<usize>,
    /// Probability that a copy of the base repeats verbatim, in `[0, 1]`.
    /// `1.0` = every copy identical; `0.0` = every copy is varied.
    pub repeatability: f64,
    /// Probability that a bar *inside a varied copy* is mutated, in `[0, 1]`.
    /// Mutation transposes the bar by the copy's interval, preserving rhythm —
    /// `A A' A''`, not new material.
    pub variation_rate: f64,
}

/// Everything one structured generation pass needs: an S6 request shape plus a
/// [`StructureControl`].
#[derive(Debug, Clone)]
pub struct StructuredRequest {
    /// Deterministic seed, shared by the S6 base pass and the vary decisions.
    pub seed: GenerationSeed,
    /// Scale to draw pitches from.
    pub pitch_material: PitchMaterial,
    /// Structural constraints; `bar_count` is the **target span** in bars.
    pub constraints: GenerationConstraints,
    /// Rhythm templates for strategies that need them.
    pub source_rhythms: Vec<Vec<Ticks>>,
    /// S6 strategy used to generate the base motif.
    pub strategy: GenerationStrategy,
    /// The structure control to compile.
    pub control: StructureControl,
}

/// A produced span plus provenance: the control that asked for it and the
/// metrics measuring what was actually produced (ADR-0015 §2).
#[derive(Debug, Clone)]
pub struct StructuredCandidate {
    /// The generated score (one track, one voice).
    pub score: Score,
    /// Seed used for this pass.
    pub seed: GenerationSeed,
    /// The control this candidate was generated against.
    pub control: StructureControl,
    /// Measured structure of the produced span — what it *is*, not what was
    /// asked; Phase 2 reranks candidates by the distance between the two.
    pub metrics: StructureMetrics,
}

/// Errors the structure compiler can emit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StructureGenError {
    /// The control is out of range: a zero or span-exceeding pattern period, or
    /// a non-finite / out-of-`[0, 1]` repeatability or variation rate.
    InvalidControl,
    /// The underlying S6 generator rejected the derived request.
    Generation(GenerationError),
    /// Measuring the produced span failed (empty span).
    Measurement(StructureError),
}

/// Generates a span under a [`StructureControl`]: base motif via S6, tiling
/// across the target span, deterministic variation, and measured provenance.
///
/// Fully deterministic: the same `request` always produces the same candidate.
pub fn generate_structured(
    request: &StructuredRequest,
) -> Result<StructuredCandidate, StructureGenError> {
    let control = request.control;
    if !in_unit_range(control.repeatability) || !in_unit_range(control.variation_rate) {
        return Err(StructureGenError::InvalidControl);
    }

    let score = match control.pattern_period_bars {
        // Through-composed: plain S6 over the whole span, no tiling.
        None => run_s6(request, request.constraints.bar_count)?,
        Some(period) => {
            if period == 0 || period > request.constraints.bar_count {
                return Err(StructureGenError::InvalidControl);
            }
            let base = run_s6(request, period)?;
            tile_and_vary(&base, request, period)?
        }
    };

    let metrics = measure_structure(&score, 0).map_err(StructureGenError::Measurement)?;
    Ok(StructuredCandidate {
        score,
        seed: request.seed,
        control,
        metrics,
    })
}

/// `true` when `x` is a finite value in `[0, 1]`.
fn in_unit_range(x: f64) -> bool {
    x.is_finite() && (0.0..=1.0).contains(&x)
}

/// Runs the S6 generator for `bar_count` bars with the request's material.
fn run_s6(request: &StructuredRequest, bar_count: usize) -> Result<Score, StructureGenError> {
    let candidate = generate(&RuleGenerationRequest {
        seed: request.seed,
        pitch_material: request.pitch_material.clone(),
        constraints: GenerationConstraints {
            bar_count,
            ..request.constraints
        },
        source_rhythms: request.source_rhythms.clone(),
        strategy: request.strategy,
    })
    .map_err(StructureGenError::Generation)?;
    Ok(candidate.score)
}

/// Tiles the `period`-bar base score across the target span, varying copies.
///
/// Bar `i` of the output copies base bar `i % period`, shifted in time. Copy
/// `k = i / period` repeats verbatim when `k == 0` or the seed-deterministic
/// per-copy draw lands under `repeatability`; otherwise each of its bars is
/// transposed by the copy's interval when the per-bar draw lands under
/// `variation_rate`. A truncated final copy restarts from the base's first bar.
fn tile_and_vary(
    base: &Score,
    request: &StructuredRequest,
    period: usize,
) -> Result<Score, StructureGenError> {
    let c = &request.constraints;
    let control = request.control;
    let seed = request.seed.0;
    let invalid = || StructureGenError::Generation(GenerationError::InvalidConstraints);

    let bar_dur =
        bar_duration_ticks(c.time_signature, c.ticks_per_quarter).map_err(|_| invalid())?;
    let per_bar = base_bars_relative(base);
    let interval_offset = usize::try_from(mix(seed, INTERVAL_SALT)).unwrap_or(0);

    let mut master_bars = Vec::with_capacity(c.bar_count);
    let mut event_groups = Vec::new();

    for i in 0..c.bar_count {
        let i_u32 = u32::try_from(i).map_err(|_| invalid())?;
        let start = i_u32.checked_mul(bar_dur.0).ok_or_else(invalid)?;
        let end = start.checked_add(bar_dur.0).ok_or_else(invalid)?;
        master_bars.push(MasterBar {
            index: i,
            tick_range: TickRange::new(Ticks(start), Ticks(end)).map_err(|_| invalid())?,
            time_signature: c.time_signature,
            tempo: c.tempo,
        });

        let j = i.checked_rem(period).unwrap_or(0);
        let k = i.checked_div(period).unwrap_or(0);
        let verbatim = k == 0 || hash_unit(seed, COPY_SALT ^ salt_of(k)) < control.repeatability;
        let interval = if verbatim {
            0
        } else {
            let idx = interval_offset
                .wrapping_add(k)
                .checked_rem(VARIATION_INTERVALS.len())
                .unwrap_or(0);
            VARIATION_INTERVALS.get(idx).copied().unwrap_or(0)
        };

        let Some(bar_notes) = per_bar.get(j) else {
            continue;
        };
        for note in bar_notes {
            let onset = start
                .checked_add(note.absolute_start.0)
                .ok_or_else(invalid)?;
            let mutate =
                interval != 0 && hash_unit(seed, BAR_SALT ^ salt_of(i)) < control.variation_rate;
            let mut out = *note;
            out.absolute_start = Ticks(onset);
            if mutate {
                out.pitch = transpose_clamped(note.pitch, interval, c.pitch_lo, c.pitch_hi);
                // A transposed pitch invalidates any carried fretboard position.
                out.position = None;
            }
            event_groups.push(EventGroup {
                kind: EventGroupKind::Single,
                atoms: vec![AtomEvent::Note(out)],
                technique_spans: Vec::new(),
            });
        }
    }

    Ok(Score {
        ticks_per_quarter: base.ticks_per_quarter,
        master_bars,
        tracks: vec![Track {
            name: None,
            channel: 0,
            voices: vec![Voice {
                id: 0,
                event_groups,
            }],
            tuning: Tuning::standard_e(),
        }],
        source_meta: None,
        loss: LossReport::new(),
    })
}

/// The base score's notes bucketed per base bar, onsets kept bar-relative,
/// sorted by onset within each bar. Reads the first track only (the S6 output
/// shape: one track, one voice).
fn base_bars_relative(base: &Score) -> Vec<Vec<AtomNote>> {
    base.master_bars
        .iter()
        .map(|mb| {
            let mut bar_notes: Vec<AtomNote> = base
                .tracks
                .iter()
                .take(1)
                .flat_map(|t| &t.voices)
                .flat_map(|v| &v.event_groups)
                .flat_map(|g| &g.atoms)
                .filter_map(|a| match a {
                    AtomEvent::Note(n)
                        if n.absolute_start.0 >= mb.tick_range.start.0
                            && n.absolute_start.0 < mb.tick_range.end.0 =>
                    {
                        let mut note = *n;
                        note.absolute_start =
                            Ticks(n.absolute_start.0.saturating_sub(mb.tick_range.start.0));
                        Some(note)
                    }
                    _ => None,
                })
                .collect();
            bar_notes.sort_by_key(|n| n.absolute_start.0);
            bar_notes
        })
        .collect()
}

/// Transposes a pitch by `semitones`, clamping into `[lo, hi]` (ordered).
fn transpose_clamped(pitch: Pitch, semitones: i32, lo: Pitch, hi: Pitch) -> Pitch {
    let raw = i32::from(pitch.0).saturating_add(semitones);
    let actual_lo = i32::from(lo.0.min(hi.0));
    let actual_hi = i32::from(lo.0.max(hi.0).min(127));
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    Pitch(raw.clamp(actual_lo, actual_hi) as u8)
}

/// Index folded into a salt; saturates rather than wrapping into a collision.
fn salt_of(index: usize) -> u64 {
    u64::try_from(index)
        .unwrap_or(u64::MAX)
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
}

/// `SplitMix64` finalizer over `(seed, salt)` — the same deterministic mixing
/// the complement module uses for its seed-derived choices.
const fn mix(seed: u64, salt: u64) -> u64 {
    let mut z = seed.wrapping_add(salt);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Maps `(seed, salt)` to a uniform value in `[0, 1)`.
#[allow(clippy::cast_precision_loss)] // top 53 bits → f64 is lossless
fn hash_unit(seed: u64, salt: u64) -> f64 {
    const TWO_POW_53: f64 = 9_007_199_254_740_992.0;
    ((mix(seed, salt) >> 11) as f64) / TWO_POW_53
}

// ── S14 Phase 2: scoring loop — rerank by control ↔ metrics distance ──────────

// Stable structure-axis labels — the join keys between [`structure_axes`] facts
// and a scoring [`WeightPolicy`] (ADR-0017). Named once so the labels and the
// axis order cannot drift apart.
const AXIS_PERIOD_MATCH: &str = "period_match";
const AXIS_REPEATABILITY_MATCH: &str = "repeatability_match";
const AXIS_VARIATION_MATCH: &str = "variation_match";

/// The structure agreement axes, in their canonical order (ADR-0017).
pub const STRUCTURE_AXIS_LABELS: [&str; 3] = [
    AXIS_PERIOD_MATCH,
    AXIS_REPEATABILITY_MATCH,
    AXIS_VARIATION_MATCH,
];

/// Salt for deriving per-candidate seeds in [`generate_structured_set`].
const CANDIDATE_SALT: u64 = 0x4341_4E44_4944_4154; // "CANDIDAT"

/// How well `metrics` agree with what `control` asked for, as labelled scoring
/// facts in `[0, 1]` (ADR-0017) — higher is closer.
///
/// - `period_match` — `1.0` when the detected period equals the requested one
///   (or both are through-composed); the `min/max` ratio for a wrong period
///   (off by 2× = `0.5`); `0.0` when one side is periodic and the other not.
/// - `repeatability_match` / `variation_match` — `1 − |requested − measured|`.
///   The knobs and the scores are different quantities (a per-copy / per-bar
///   probability vs a mean self-similarity), but both are normalised monotone
///   proxies for the same axis; the absolute distance is the honest v1 measure.
#[must_use]
#[allow(clippy::cast_precision_loss)] // bar periods are tiny; no precision concern
pub fn structure_axes(control: &StructureControl, metrics: &StructureMetrics) -> Axes {
    let period_match = match (
        control.pattern_period_bars,
        metrics.detected_pattern_period_bars,
    ) {
        (None, None) => 1.0,
        (None, Some(_)) | (Some(_), None) => 0.0,
        (Some(requested), Some(detected)) => {
            let lo = requested.min(detected);
            let hi = requested.max(detected);
            if hi == 0 {
                0.0
            } else {
                lo as f64 / hi as f64
            }
        }
    };
    let repeatability_match = 1.0
        - (control.repeatability - metrics.repeatability_score)
            .abs()
            .clamp(0.0, 1.0);
    let variation_match = 1.0
        - (control.variation_rate - metrics.variation_score)
            .abs()
            .clamp(0.0, 1.0);

    Axes::new(vec![
        Axis {
            label: AXIS_PERIOD_MATCH,
            value: period_match,
        },
        Axis {
            label: AXIS_REPEATABILITY_MATCH,
            value: repeatability_match,
        },
        Axis {
            label: AXIS_VARIATION_MATCH,
            value: variation_match,
        },
    ])
}

/// The baseline structure weight policy (`structure` v1): uniform over the
/// three agreement axes.
///
/// Untuned by design — weights are *data* the feedback layer (S9) learns
/// (ADR-0017 §3). Under the uniform policy the aggregate reads as "how closely
/// the produced span matches the requested structure".
#[must_use]
pub fn structure_weights_v1() -> WeightPolicy {
    WeightPolicy::uniform("structure", 1, &STRUCTURE_AXIS_LABELS)
}

impl StructuredCandidate {
    /// An explainable [`Scored`] view of this candidate under `policy`
    /// (ADR-0017): how well its measured metrics hit its control.
    ///
    /// The value is the candidate's seed; the provenance carries that seed and
    /// the weight-policy version, so the aggregate and any ranking are
    /// reproducible relative to `(seed, policy version)` (ADR-0017 §7).
    #[must_use]
    pub fn scored(&self, policy: &WeightPolicy) -> Scored<GenerationSeed> {
        Scored::new(
            self.seed,
            structure_axes(&self.control, &self.metrics),
            policy,
            Some(self.seed.0),
        )
    }
}

/// Ranks structure candidates under `policy`, closest-to-request first, ties
/// broken by candidate order (the fixed tie-break rule, ADR-0017 §7).
///
/// Returns indices into `candidates`. Deterministic for fixed inputs and a
/// fixed policy version. Rejection is the caller's cut: take the top of the
/// ranking, or filter by aggregate threshold.
#[must_use]
pub fn rank_structured(candidates: &[StructuredCandidate], policy: &WeightPolicy) -> Vec<usize> {
    let scored: Vec<Scored<GenerationSeed>> = candidates.iter().map(|c| c.scored(policy)).collect();
    rank_indices(&scored)
}

/// Generates `count` candidates for one request.
///
/// Candidate 0 uses the request seed as-is (so the set extends, not replaces,
/// the single pass); each further candidate uses a seed derived
/// deterministically from it.
///
/// The whole set is deterministic for a fixed request; rerank it with
/// [`rank_structured`] and keep the best (the Phase-2 scoring loop, ADR-0015).
pub fn generate_structured_set(
    request: &StructuredRequest,
    count: usize,
) -> Result<Vec<StructuredCandidate>, StructureGenError> {
    let mut candidates = Vec::with_capacity(count);
    for i in 0..count {
        let seed = if i == 0 {
            request.seed
        } else {
            GenerationSeed(mix(request.seed.0, CANDIDATE_SALT ^ salt_of(i)))
        };
        let mut derived = request.clone();
        derived.seed = seed;
        candidates.push(generate_structured(&derived)?);
    }
    Ok(candidates)
}
