//! Cutting a track's bars into phrase-aligned segments for `griff split`.
//!
//! [`bar_segments`] turns phrase-boundary onset ticks into contiguous,
//! non-overlapping bar ranges covering the whole track — one per phrase. Paired
//! with [`crate::slice::extract_bars`] it yields one standalone, measurable score
//! per phrase, which curation writes as one chunk each.

use std::collections::BTreeSet;
use std::ops::Range;

use crate::score::MasterBar;

/// Default upper bound, in bars, on a single phrase segment.
///
/// Segments longer than this are subdivided by [`cap_segment_bars`] so curation
/// never faces a 30-bar blob spanning several rhythmic patterns (#76).
/// Deliberately generous: it splits only clearly over-long phrases, leaving
/// ordinary 4–16-bar ones whole — a sensible default, not a tuned constant.
pub const MAX_PHRASE_BARS: usize = 16;

/// Partitions `master_bars` into contiguous bar ranges cut at `cut_ticks`.
///
/// Each cut tick is snapped to the bar that contains it; a cut at bar 0 (or the
/// track start) is a no-op, since the first segment always starts at bar 0.
/// Cuts in the same bar collapse to one. The returned ranges are sorted,
/// non-overlapping, and cover every bar, so the segments reassemble the whole
/// track. Empty when there are no bars.
#[must_use]
pub fn bar_segments(master_bars: &[MasterBar], cut_ticks: &[u32]) -> Vec<Range<usize>> {
    let bar_count = master_bars.len();
    if bar_count == 0 {
        return Vec::new();
    }
    let mut cuts: BTreeSet<usize> = BTreeSet::new();
    cuts.insert(0);
    cuts.insert(bar_count);
    for &tick in cut_ticks {
        match bar_containing(master_bars, tick) {
            Some(idx) if idx > 0 => {
                cuts.insert(idx);
            }
            _ => {}
        }
    }
    cuts.iter()
        .zip(cuts.iter().skip(1))
        .map(|(&start, &end)| start..end)
        .collect()
}

/// Subdivides any segment spanning more than `max_bars` bars into consecutive
/// sub-ranges of at most `max_bars` bars each, leaving shorter segments intact.
///
/// `max_bars == 0` disables the cap. Like [`bar_segments`] the result is sorted,
/// non-overlapping, and covers exactly the input bars: it only adds cuts, so a
/// phrase the detector left over-long becomes several measurable sub-phrases
/// (#76) instead of one blob. The trailing sub-range carries the remainder.
#[must_use]
pub fn cap_segment_bars(segments: &[Range<usize>], max_bars: usize) -> Vec<Range<usize>> {
    if max_bars == 0 {
        return segments.to_vec();
    }
    let mut out: Vec<Range<usize>> = Vec::with_capacity(segments.len());
    for seg in segments {
        let mut start = seg.start;
        // Emit full cap-sized blocks while more than a cap remains, so the final
        // piece is the leftover (1..=max_bars bars), never an empty range.
        while seg.end.saturating_sub(start) > max_bars {
            let next = start.saturating_add(max_bars);
            out.push(start..next);
            start = next;
        }
        if start < seg.end {
            out.push(start..seg.end);
        }
    }
    out
}

/// Index of the bar whose half-open tick range contains `tick`.
fn bar_containing(master_bars: &[MasterBar], tick: u32) -> Option<usize> {
    master_bars
        .iter()
        .position(|b| tick >= b.tick_range.start.0 && tick < b.tick_range.end.0)
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::arithmetic_side_effects,
        clippy::single_range_in_vec_init
    )]

    use super::{bar_segments, cap_segment_bars};
    use crate::event::{Tempo, Ticks, TimeSignature};
    use crate::score::{MasterBar, RepeatMarker};
    use crate::slice::TickRange;

    /// `n` consecutive 4/4 bars of 1920 ticks each.
    fn bars(n: usize) -> Vec<MasterBar> {
        let mut out = Vec::new();
        let mut start = 0_u32;
        for index in 0..n {
            out.push(MasterBar {
                index,
                tick_range: TickRange::new(Ticks(start), Ticks(start + 1920)).expect("ordered"),
                time_signature: TimeSignature {
                    numerator: 4,
                    denominator: 4,
                },
                tempo: Tempo::new(120.0).expect("bpm"),
                repeat: RepeatMarker::default(),
            });
            start += 1920;
        }
        out
    }

    #[test]
    fn no_cuts_yield_one_whole_track_segment() {
        assert_eq!(bar_segments(&bars(4), &[]), vec![0..4]);
    }

    #[test]
    fn cuts_snap_to_their_containing_bar() {
        // 3840 is bar 2's downbeat; 1920 is bar 1's, 5760 is bar 3's.
        assert_eq!(bar_segments(&bars(4), &[3840]), vec![0..2, 2..4]);
        assert_eq!(bar_segments(&bars(4), &[1920, 5760]), vec![0..1, 1..3, 3..4]);
    }

    #[test]
    fn start_cut_is_a_noop_and_same_bar_cuts_collapse() {
        assert_eq!(bar_segments(&bars(4), &[0]), vec![0..4]);
        // 3840 and 3850 share bar 2 → a single cut.
        assert_eq!(bar_segments(&bars(4), &[3840, 3850]), vec![0..2, 2..4]);
    }

    #[test]
    fn no_bars_yield_no_segments() {
        assert!(bar_segments(&[], &[1920]).is_empty());
    }

    #[test]
    fn cap_zero_leaves_segments_unchanged() {
        assert_eq!(cap_segment_bars(&[0..30], 0), vec![0..30]);
        assert_eq!(cap_segment_bars(&[0..4, 4..9], 0), vec![0..4, 4..9]);
    }

    #[test]
    fn segments_within_the_cap_are_kept_whole() {
        assert_eq!(cap_segment_bars(&[0..16], 16), vec![0..16]); // == cap
        assert_eq!(cap_segment_bars(&[0..5], 16), vec![0..5]); // < cap
    }

    #[test]
    fn over_long_segments_split_into_cap_blocks_plus_remainder() {
        assert_eq!(cap_segment_bars(&[0..30], 16), vec![0..16, 16..30]);
        // Exact multiple → even blocks, no stray empty range.
        assert_eq!(cap_segment_bars(&[0..32], 16), vec![0..16, 16..32]);
        // A non-zero offset is preserved as the sub-ranges advance.
        assert_eq!(cap_segment_bars(&[4..40], 16), vec![4..20, 20..36, 36..40]);
    }

    #[test]
    fn caps_each_segment_independently() {
        assert_eq!(
            cap_segment_bars(&[0..4, 4..40], 16),
            vec![0..4, 4..20, 20..36, 36..40]
        );
    }
}
