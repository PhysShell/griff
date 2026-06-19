//! Novelty guard v1 — does a candidate quote reference material verbatim?
//! (ADR-0017; `docs/audit/2026-06-melodic-closure-research.md` §3.4, §7.3.)
//!
//! The product requirement it enforces: *learn schemata from real songs,
//! never emit their fragments*. The guard compares a candidate track against
//! reference scores (corpus chunks) over **transition sequences**: each
//! element is `(pitch interval, normalised inter-onset interval)` between
//! successive notes of the highest-pitch-per-onset melodic line. Comparing
//! transitions rather than notes makes a quote visible through transposition
//! (intervals are pitch-relative) and through tick-resolution changes (IOIs
//! are rescaled to a common grid).
//!
//! Two standard originality measures from the generation-evaluation
//! literature (research note §8) become ADR-0017 facts via [`novelty_axes`]:
//!
//! - `quote_novelty` — the share of the candidate *not* covered by its
//!   longest verbatim quote (longest common contiguous transition run);
//! - `ngram_novelty` — the share of the candidate's transition n-grams
//!   absent from every reference.
//!
//! [`measure_novelty`] returns the raw [`NoveltyReport`] so the caller can
//! apply its own threshold cut (e.g. reject quotes longer than 1–2 bars);
//! rejection thresholds are the caller's policy, not code (ADR-0017 spirit).
//!
//! Conventions: the candidate reads the first voice of the given track; each
//! reference contributes its first note-bearing track (first voice) — the
//! same conventions as the closure scorer and `griff curate`. The run search
//! is a direct O(n·m·len) scan, fine at micro-corpus scale; a suffix
//! automaton is a parked optimisation. Pure and deterministic (SPEC §6).

use std::collections::BTreeSet;

use crate::score::{AtomEvent, LossReport, Score, Track};
use crate::scoring::{Axes, Axis, WeightPolicy};

const AXIS_QUOTE_NOVELTY: &str = "quote_novelty";
const AXIS_NGRAM_NOVELTY: &str = "ngram_novelty";

/// The novelty axes, in their canonical order (ADR-0017).
pub const NOVELTY_AXIS_LABELS: [&str; 2] = [AXIS_QUOTE_NOVELTY, AXIS_NGRAM_NOVELTY];

/// Transitions per n-gram window (four transitions ≈ a five-note figure).
const NGRAM_TRANSITIONS: usize = 4;

/// The grid inter-onset intervals are rescaled to (ticks per quarter), so
/// scores at different resolutions compare equal.
const IOI_GRID_PER_QUARTER: u32 = 480;

/// Errors the novelty guard can emit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoveltyError {
    /// The candidate track index is out of range.
    TrackIndexOutOfRange,
    /// The candidate track's first voice contains no notes.
    NoNotes,
}

/// Raw novelty facts for one candidate against a reference set.
///
/// Note counts refer to the melodic line (one note per onset: a chord is its
/// top note). The derived shares live in [`novelty_axes`]; the raw fields are
/// the caller's threshold surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NoveltyReport {
    /// Line notes in the candidate.
    pub candidate_notes: usize,
    /// Line notes covered by the longest verbatim quote (`0` when no
    /// transition is shared; otherwise ≥ 2).
    pub longest_match_notes: usize,
    /// Index into `references` of the quote's source — the first reference
    /// achieving the longest run (fixed tie-break) — or `None` when nothing
    /// matches.
    pub longest_match_reference: Option<usize>,
    /// Transitions per n-gram window ([`NGRAM_TRANSITIONS`]).
    pub ngram_size: usize,
    /// N-gram windows in the candidate.
    pub candidate_ngrams: usize,
    /// Candidate windows found verbatim in any reference.
    pub matched_ngrams: usize,
}

/// One melodic step: `(pitch interval in semitones, IOI on the common grid)`.
type Transition = (i16, u32);

/// Measures how much of the candidate track is quoted from `references`.
///
/// An empty reference set (no corpus yet) reports full novelty. Deterministic
/// for fixed inputs.
pub fn measure_novelty(
    score: &Score,
    track_index: usize,
    references: &[Score],
) -> Result<NoveltyReport, NoveltyError> {
    let track = score
        .tracks
        .get(track_index)
        .ok_or(NoveltyError::TrackIndexOutOfRange)?;
    let line = top_line(track);
    if line.is_empty() {
        return Err(NoveltyError::NoNotes);
    }
    let candidate = transitions(&line, score.ticks_per_quarter);

    let reference_seqs: Vec<(usize, Vec<Transition>)> = references
        .iter()
        .enumerate()
        .filter_map(|(index, reference)| {
            let reference_line = reference
                .tracks
                .iter()
                .map(top_line)
                .find(|l| !l.is_empty())?;
            let seq = transitions(&reference_line, reference.ticks_per_quarter);
            if seq.is_empty() {
                None
            } else {
                Some((index, seq))
            }
        })
        .collect();

    let mut longest = 0_usize;
    let mut longest_reference = None;
    for (index, seq) in &reference_seqs {
        let run = longest_common_run(&candidate, seq);
        if run > longest {
            longest = run;
            longest_reference = Some(*index);
        }
    }

    let reference_ngrams: BTreeSet<&[Transition]> = reference_seqs
        .iter()
        .flat_map(|(_, seq)| seq.windows(NGRAM_TRANSITIONS))
        .collect();
    let candidate_windows = candidate.windows(NGRAM_TRANSITIONS);
    let candidate_ngrams = candidate_windows.len();
    let matched_ngrams = candidate_windows
        .filter(|w| reference_ngrams.contains(w))
        .count();

    Ok(NoveltyReport {
        candidate_notes: line.len(),
        longest_match_notes: if longest == 0 {
            0
        } else {
            longest.saturating_add(1)
        },
        longest_match_reference: longest_reference,
        ngram_size: NGRAM_TRANSITIONS,
        candidate_ngrams,
        matched_ngrams,
    })
}

/// The novelty facts of a report, as labelled axes in `[0, 1]` (ADR-0017):
/// the *free* share of the candidate, so higher is more novel.
///
/// A candidate with nothing to quote (fewer than two line notes, or no
/// n-gram window) reads as fully novel on the respective axis.
#[must_use]
pub fn novelty_axes(report: &NoveltyReport) -> Axes {
    let candidate_transitions = report.candidate_notes.saturating_sub(1);
    let quote_transitions = report.longest_match_notes.saturating_sub(1);
    let quote_novelty = free_share(candidate_transitions, quote_transitions);
    let ngram_novelty = free_share(report.candidate_ngrams, report.matched_ngrams);

    Axes::new(vec![
        Axis {
            label: AXIS_QUOTE_NOVELTY,
            value: quote_novelty,
        },
        Axis {
            label: AXIS_NGRAM_NOVELTY,
            value: ngram_novelty,
        },
    ])
}

/// The baseline novelty weight policy (`novelty` v1): uniform over the two
/// axes. Untuned by design — weights are data the feedback layer (S9) learns
/// (ADR-0017 §3).
#[must_use]
pub fn novelty_weights_v1() -> WeightPolicy {
    WeightPolicy::uniform("novelty", 1, &NOVELTY_AXIS_LABELS)
}

/// Default minimum verbatim-quote share for flagging a phrase as a near-
/// duplicate of an earlier one (#76).
///
/// A chorus/verse repeat quotes almost all of an earlier phrase; a distinct
/// phrase shares at most a short motif, so a high bar keeps false positives low.
pub const PHRASE_DUPLICATE_SHARE: f64 = 0.8;

/// A phrase flagged as a near-duplicate of an earlier one (#76).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PhraseDuplicate {
    /// Index, within the phrase list, of the earlier phrase it most closely quotes.
    pub of: usize,
    /// Share of this phrase's melodic line covered by that verbatim quote, in
    /// `[0, 1]` — `1.0` is an exact (possibly transposed) repeat.
    pub quote_share: f64,
}

/// Flags each phrase that near-duplicates an *earlier* one in `phrases` (#76).
///
/// For phrase *i*, compares its `track_index` line against phrases `0..i` with
/// [`measure_novelty`] (transposition- and resolution-aware); when the longest
/// verbatim quote covers at least `min_quote_share` of phrase *i*'s notes, it is
/// flagged a near-duplicate of the earlier phrase that quote comes from. The
/// first occurrence of a repeated phrase is canonical (never flagged); only
/// later repeats are. Returns one entry per phrase (`None` = distinct enough).
///
/// Both sides are reduced to `track_index` first, so a reference resolves to the
/// same musical line even when an earlier track also sounds in that phrase.
/// Curation surfaces the flag; whether to drop the repeat stays the curator's
/// call — the guard measures, the caller decides (ADR-0017 spirit).
#[must_use]
pub fn flag_phrase_duplicates(
    phrases: &[Score],
    track_index: usize,
    min_quote_share: f64,
) -> Vec<Option<PhraseDuplicate>> {
    let lines: Vec<Score> = phrases
        .iter()
        .map(|p| single_track_line(p, track_index))
        .collect();
    lines
        .iter()
        .enumerate()
        .map(|(i, candidate)| {
            let earlier = lines.get(..i).unwrap_or(&[]);
            match measure_novelty(candidate, 0, earlier) {
                Ok(report) if report.candidate_notes > 0 => {
                    // Reason: note counts are tiny relative to f64 mantissa precision.
                    #[allow(clippy::cast_precision_loss)]
                    let share = report.longest_match_notes as f64 / report.candidate_notes as f64;
                    match report.longest_match_reference {
                        Some(of) if share >= min_quote_share => {
                            Some(PhraseDuplicate { of, quote_share: share })
                        }
                        _ => None,
                    }
                }
                // Out-of-range / empty track, or no quote: not a flagged duplicate.
                _ => None,
            }
        })
        .collect()
}

/// A copy of `score` keeping only `track_index` as its sole track, so a novelty
/// comparison reads that one line on both the candidate and the references
/// (master bars are irrelevant to the transition representation).
fn single_track_line(score: &Score, track_index: usize) -> Score {
    Score {
        ticks_per_quarter: score.ticks_per_quarter,
        master_bars: Vec::new(),
        tracks: score.tracks.get(track_index).cloned().into_iter().collect(),
        source_meta: None,
        loss: LossReport::new(),
    }
}

/// `(total − taken) / total`, or `1.0` when there is no total.
///
/// Computed as a single correctly-rounded division so exact shares (`0.5`,
/// `0.8`, …) compare equal to their literals.
fn free_share(total: usize, taken: usize) -> f64 {
    if total == 0 {
        return 1.0;
    }
    let free = total.saturating_sub(taken);
    // Reason: counts are tiny relative to f64 mantissa precision.
    #[allow(clippy::cast_precision_loss)]
    let share = free as f64 / total as f64;
    share
}

// ── line and transition extraction ────────────────────────────────────────────

/// The melodic line of the track's first voice as `(onset, top pitch)`, one
/// entry per onset, in onset order.
fn top_line(track: &Track) -> Vec<(u32, u8)> {
    let Some(voice) = track.voices.first() else {
        return Vec::new();
    };
    let mut notes: Vec<(u32, u8)> = voice
        .event_groups
        .iter()
        .flat_map(|g| &g.atoms)
        .filter_map(|a| match a {
            AtomEvent::Note(n) => Some((n.absolute_start.0, n.pitch.0)),
            AtomEvent::Rest(_) => None,
        })
        .collect();
    notes.sort_unstable();

    let mut line: Vec<(u32, u8)> = Vec::with_capacity(notes.len());
    for (onset, pitch) in notes {
        match line.last_mut() {
            Some((last_onset, last_pitch)) if *last_onset == onset => {
                *last_pitch = (*last_pitch).max(pitch);
            }
            _ => line.push((onset, pitch)),
        }
    }
    line
}

/// Successive `(interval, normalised IOI)` steps of a melodic line.
///
/// IOIs are rescaled from the score's resolution onto
/// [`IOI_GRID_PER_QUARTER`]; sub-grid remainders truncate. A zero
/// `ticks_per_quarter` (malformed score) is clamped to `1` — the guard
/// measures; it does not validate transport.
fn transitions(line: &[(u32, u8)], ticks_per_quarter: u16) -> Vec<Transition> {
    let grid = u64::from(IOI_GRID_PER_QUARTER);
    let ppqn = u64::from(ticks_per_quarter.max(1));

    line.iter()
        .zip(line.iter().skip(1))
        .map(|(&(onset_a, pitch_a), &(onset_b, pitch_b))| {
            // Reason: pitches are ≤ 127, so the difference fits i16; onsets
            // are sorted, so the IOI subtraction cannot underflow in u64.
            #[allow(clippy::arithmetic_side_effects)]
            let interval = i16::from(pitch_b) - i16::from(pitch_a);
            let ioi = u64::from(onset_b).saturating_sub(u64::from(onset_a));
            // Reason: ppqn is clamped non-zero; the product fits u64.
            #[allow(clippy::arithmetic_side_effects)]
            let ioi_norm = u32::try_from(ioi * grid / ppqn).unwrap_or(u32::MAX);
            (interval, ioi_norm)
        })
        .collect()
}

/// Length of the longest common contiguous run between two transition
/// sequences — the candidate's longest verbatim quote, in transitions.
fn longest_common_run(a: &[Transition], b: &[Transition]) -> usize {
    let mut best = 0_usize;
    for i in 0..a.len() {
        for j in 0..b.len() {
            let run = a
                .iter()
                .skip(i)
                .zip(b.iter().skip(j))
                .take_while(|(x, y)| x == y)
                .count();
            best = best.max(run);
        }
    }
    best
}
