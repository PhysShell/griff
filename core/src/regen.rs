//! Region regeneration with frozen regions (S11).
//!
//! Regenerates a selected [`TickRange`] inside a [`Phrase`] while keeping
//! any [`FrozenRegion`]s and [`AnchorPoint`]s byte-stable, then evaluates a
//! [`ContinuityCheck`] at both joins (≤ 7 semitone interval threshold).

use crate::{
    event::{Bar, Event, Phrase, Pitch, Ticks},
    generate::{
        generate, GenerationConstraints, GenerationError, GenerationSeed, GenerationStrategy,
        PitchMaterial, RuleGenerationRequest,
    },
    slice::TickRange,
};
use thiserror::Error;

// ── public types ──────────────────────────────────────────────────────────────

/// The span to regenerate, expressed as a half-open [`TickRange`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegenerationRegion {
    /// Half-open tick range: `start <= tick < end`.
    pub range: TickRange,
}

/// A sub-range that must remain byte-identical after regeneration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrozenRegion {
    /// Half-open tick range that must not be altered.
    pub range: TickRange,
}

/// A single (tick, pitch) pair that must be preserved.
///
/// Anchor points are stored on the request and available for future
/// hard-constraint generation; they do not yet gate the generation result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnchorPoint {
    /// Absolute tick position of the anchor.
    pub tick: Ticks,
    /// Required pitch at that position.
    pub pitch: Pitch,
}

/// Frozen-region and anchor constraints bundled together.
#[derive(Debug, Clone)]
pub struct RegenerationConstraints {
    /// Spans inside the region that must be kept byte-stable.
    pub frozen: Vec<FrozenRegion>,
    /// Individual (tick, pitch) events that must be preserved.
    pub anchors: Vec<AnchorPoint>,
}

/// Pass/fail continuity at each join of the regenerated region.
///
/// A join passes when the semitone interval between the adjacent notes is
/// ≤ [`MAX_SEMITONE_INTERVAL`] (7), or when one side has no notes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContinuityCheck {
    /// Whether the left (before → region) join is smooth.
    pub left_join_passes: bool,
    /// Whether the right (region → after) join is smooth.
    pub right_join_passes: bool,
    /// Semitone interval at the left join, if both sides have notes.
    pub left_interval: Option<u8>,
    /// Semitone interval at the right join, if both sides have notes.
    pub right_interval: Option<u8>,
}

impl ContinuityCheck {
    /// Returns `true` when both joins pass.
    pub const fn passes(self) -> bool {
        self.left_join_passes && self.right_join_passes
    }
}

/// Result of a successful [`regenerate`] call.
#[derive(Debug, Clone)]
pub struct RegenerationOutput {
    /// The assembled phrase with frozen parts kept and free parts replaced.
    pub phrase: Phrase,
    /// Continuity evaluation at the region boundaries.
    pub continuity: ContinuityCheck,
}

/// Error returned by [`regenerate`].
// RegionOutOfBounds (3×u32 = 12 bytes) is intentionally larger than
// GenerationFailed (1-byte unit enum); both are small value types.
#[allow(variant_size_differences)]
#[derive(Debug, Clone, Copy, Error, PartialEq, Eq)]
pub enum RegenerationError {
    /// The region extends past the end of the source phrase.
    #[error("region {start}..{end} exceeds phrase length {phrase_end} ticks")]
    RegionOutOfBounds {
        /// Region start tick.
        start: u32,
        /// Region end tick.
        end: u32,
        /// Source phrase total length in ticks.
        phrase_end: u32,
    },
    /// The underlying S6 generator rejected the request.
    #[error("generation failed: {0:?}")]
    GenerationFailed(GenerationError),
}

/// Everything needed for one regeneration pass.
#[derive(Debug, Clone)]
pub struct RegenerationRequest {
    /// Source phrase.  Bars outside `region` are copied verbatim.
    pub source: Phrase,
    /// The span to regenerate.
    pub region: RegenerationRegion,
    /// Frozen regions and anchor points inside `region`.
    pub constraints: RegenerationConstraints,
    /// Pitch scale for the S6 generator.
    pub pitch_material: PitchMaterial,
    /// Rhythm templates passed through to the generator (may be empty).
    pub source_rhythms: Vec<Vec<Ticks>>,
    /// Structural constraints for the generator (`bar_count` is overridden
    /// to match the number of free bars in the region).
    pub generation_constraints: GenerationConstraints,
    /// Which generation strategy to apply to free bars.
    pub strategy: GenerationStrategy,
    /// Deterministic PRNG seed.
    pub seed: GenerationSeed,
}

// ── constants ─────────────────────────────────────────────────────────────────

/// Maximum semitone interval at a join for the continuity check to pass.
pub const MAX_SEMITONE_INTERVAL: u8 = 7;

// ── public API ────────────────────────────────────────────────────────────────

/// Regenerates the free bars inside `req.region`, preserving frozen bars
/// byte-for-byte and evaluating continuity at both boundary joins.
pub fn regenerate(req: &RegenerationRequest) -> Result<RegenerationOutput, RegenerationError> {
    let bar_starts = compute_bar_starts(&req.source);
    let phrase_end = phrase_end_tick(&req.source);
    let region = req.region.range;

    if region.end.0 > phrase_end.0 {
        return Err(RegenerationError::RegionOutOfBounds {
            start: region.start.0,
            end: region.end.0,
            phrase_end: phrase_end.0,
        });
    }

    // Partition source bars into before / in-region (frozen flag) / after.
    let mut before = Vec::new();
    let mut in_region: Vec<(Bar, bool)> = Vec::new(); // (bar, is_frozen)
    let mut after = Vec::new();
    let mut free_count: usize = 0;

    for (i, bar) in req.source.bars.iter().enumerate() {
        let b_start = bar_starts.get(i).copied().unwrap_or(Ticks::ZERO);
        let b_end = bar_end_tick(bar, b_start);

        if b_end.0 <= region.start.0 {
            before.push(bar.clone());
        } else if b_start.0 >= region.end.0 {
            after.push(bar.clone());
        } else {
            let frozen = bar_is_frozen(b_start, b_end, &req.constraints.frozen);
            if !frozen {
                free_count = free_count.saturating_add(1);
            }
            in_region.push((bar.clone(), frozen));
        }
    }

    // Generate replacement bars for the free slots.
    let generated = if free_count > 0 {
        let gen_req = RuleGenerationRequest {
            seed: req.seed,
            pitch_material: req.pitch_material.clone(),
            constraints: GenerationConstraints {
                bar_count: free_count,
                ..req.generation_constraints
            },
            source_rhythms: req.source_rhythms.clone(),
            strategy: req.strategy,
        };
        generate(&gen_req)
            .map_err(RegenerationError::GenerationFailed)?
            .phrase
            .bars
    } else {
        Vec::new()
    };

    // Assemble output.
    let mut output_bars: Vec<Bar> = before;
    let mut gen_iter = generated.into_iter();
    for (bar, frozen) in in_region {
        if frozen {
            output_bars.push(bar);
        } else {
            // Fallback to original bar if generator produced fewer bars than
            // expected (should not happen but prevents a panic).
            output_bars.push(gen_iter.next().unwrap_or(bar));
        }
    }
    output_bars.extend(after);

    let phrase = Phrase { bars: output_bars };
    let continuity = check_continuity(&phrase, region);

    Ok(RegenerationOutput { phrase, continuity })
}

// ── private helpers ───────────────────────────────────────────────────────────

/// Returns the absolute start tick of each bar in `phrase`.
fn compute_bar_starts(phrase: &Phrase) -> Vec<Ticks> {
    let mut starts = Vec::with_capacity(phrase.bars.len());
    let mut cursor = Ticks::ZERO;
    for bar in &phrase.bars {
        starts.push(cursor);
        let dur = bar.duration().unwrap_or(Ticks::ZERO);
        cursor = cursor.checked_add(dur).unwrap_or(cursor);
    }
    starts
}

/// Returns the tick at which `phrase` ends (sum of all bar durations).
fn phrase_end_tick(phrase: &Phrase) -> Ticks {
    let mut cursor = Ticks::ZERO;
    for bar in &phrase.bars {
        let dur = bar.duration().unwrap_or(Ticks::ZERO);
        cursor = cursor.checked_add(dur).unwrap_or(cursor);
    }
    cursor
}

/// Returns the exclusive end tick of `bar` given its start.
fn bar_end_tick(bar: &Bar, start: Ticks) -> Ticks {
    let dur = bar.duration().unwrap_or(Ticks::ZERO);
    start.checked_add(dur).unwrap_or(start)
}

/// Returns `true` when `[b_start, b_end)` is fully covered by any frozen region.
fn bar_is_frozen(b_start: Ticks, b_end: Ticks, frozen: &[FrozenRegion]) -> bool {
    frozen
        .iter()
        .any(|f| f.range.start.0 <= b_start.0 && b_end.0 <= f.range.end.0)
}

/// Evaluates the continuity check for the assembled `phrase` at `region`.
fn check_continuity(phrase: &Phrase, region: TickRange) -> ContinuityCheck {
    let before_pitch = last_note_before(phrase, region.start);
    let region_first = first_note_in_range(phrase, region);
    let region_last = last_note_in_range(phrase, region);
    let after_pitch = first_note_at_or_after(phrase, region.end);

    let left_interval = before_pitch.zip(region_first).map(|(a, b)| a.abs_diff(b));
    let right_interval = region_last.zip(after_pitch).map(|(a, b)| a.abs_diff(b));

    ContinuityCheck {
        left_join_passes: left_interval.map_or(true, |i| i <= MAX_SEMITONE_INTERVAL),
        right_join_passes: right_interval.map_or(true, |i| i <= MAX_SEMITONE_INTERVAL),
        left_interval,
        right_interval,
    }
}

/// Returns the pitch of the last note whose absolute start tick is `< tick`.
fn last_note_before(phrase: &Phrase, tick: Ticks) -> Option<u8> {
    let mut result = None;
    let mut cursor = Ticks::ZERO;
    for bar in &phrase.bars {
        let mut ev_cursor = cursor;
        for event in &bar.events {
            if ev_cursor.0 < tick.0 {
                if let Event::Note(n) = event {
                    result = Some(n.pitch.0);
                }
            }
            ev_cursor = ev_cursor
                .checked_add(event.duration())
                .unwrap_or(ev_cursor);
        }
        cursor = cursor
            .checked_add(bar.duration().unwrap_or(Ticks::ZERO))
            .unwrap_or(cursor);
    }
    result
}

/// Returns the pitch of the first note whose absolute start tick is
/// `>= range.start` and `< range.end`.
fn first_note_in_range(phrase: &Phrase, range: TickRange) -> Option<u8> {
    let mut cursor = Ticks::ZERO;
    for bar in &phrase.bars {
        let mut ev_cursor = cursor;
        for event in &bar.events {
            if ev_cursor.0 >= range.start.0 && ev_cursor.0 < range.end.0 {
                if let Event::Note(n) = event {
                    return Some(n.pitch.0);
                }
            }
            ev_cursor = ev_cursor
                .checked_add(event.duration())
                .unwrap_or(ev_cursor);
        }
        cursor = cursor
            .checked_add(bar.duration().unwrap_or(Ticks::ZERO))
            .unwrap_or(cursor);
    }
    None
}

/// Returns the pitch of the last note whose absolute start tick is
/// `>= range.start` and `< range.end`.
fn last_note_in_range(phrase: &Phrase, range: TickRange) -> Option<u8> {
    let mut result = None;
    let mut cursor = Ticks::ZERO;
    for bar in &phrase.bars {
        let mut ev_cursor = cursor;
        for event in &bar.events {
            if ev_cursor.0 >= range.start.0 && ev_cursor.0 < range.end.0 {
                if let Event::Note(n) = event {
                    result = Some(n.pitch.0);
                }
            }
            ev_cursor = ev_cursor
                .checked_add(event.duration())
                .unwrap_or(ev_cursor);
        }
        cursor = cursor
            .checked_add(bar.duration().unwrap_or(Ticks::ZERO))
            .unwrap_or(cursor);
    }
    result
}

/// Returns the pitch of the first note whose absolute start tick is `>= tick`.
fn first_note_at_or_after(phrase: &Phrase, tick: Ticks) -> Option<u8> {
    let mut cursor = Ticks::ZERO;
    for bar in &phrase.bars {
        let mut ev_cursor = cursor;
        for event in &bar.events {
            if ev_cursor.0 >= tick.0 {
                if let Event::Note(n) = event {
                    return Some(n.pitch.0);
                }
            }
            ev_cursor = ev_cursor
                .checked_add(event.duration())
                .unwrap_or(ev_cursor);
        }
        cursor = cursor
            .checked_add(bar.duration().unwrap_or(Ticks::ZERO))
            .unwrap_or(cursor);
    }
    None
}
