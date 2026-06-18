//! Cutting a track's bars into phrase-aligned segments for `griff split`.
//!
//! [`bar_segments`] turns phrase-boundary onset ticks into contiguous,
//! non-overlapping bar ranges covering the whole track — one per phrase. Paired
//! with [`crate::slice::extract_bars`] it yields one standalone, measurable score
//! per phrase, which curation writes as one chunk each.

use std::collections::BTreeSet;
use std::ops::Range;

use crate::score::MasterBar;

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

/// Index of the bar whose half-open tick range contains `tick`.
fn bar_containing(master_bars: &[MasterBar], tick: u32) -> Option<usize> {
    master_bars
        .iter()
        .position(|b| tick >= b.tick_range.start.0 && tick < b.tick_range.end.0)
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, clippy::arithmetic_side_effects)]

    use super::bar_segments;
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
}
