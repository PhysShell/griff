//! Repeat unfolding — the played-order projection of a written [`Score`]
//! (ADR-0022).
//!
//! The canonical model stays faithful to the source: simple repeats survive as
//! [`RepeatMarker`](crate::score::RepeatMarker)s on each
//! [`MasterBar`](crate::score::MasterBar), *folded*. This projection expands
//! them on demand into the linear sequence of bars as actually played. Only
//! simple `|: … :|×N` repeats are modeled; alternate endings (voltas) and jump
//! directions (D.C./D.S.) are treated as plain barlines — a documented S3
//! limitation, not silent data loss (the markers remain on the model).

use crate::score::Score;

/// Hard ceiling on expansion, as a multiple of the written bar count, so a
/// malformed repeat map cannot allocate without bound. Monotonic per-close pass
/// counting already guarantees termination; this is fuzz belt-and-braces.
const MAX_EXPANSION: usize = 64;

/// Returns the played order of master-bar indices after expanding simple
/// repeats.
///
/// Each element indexes into [`Score::master_bars`]. A score with no repeat
/// barlines yields `0..master_bars.len()`. A `|:` opens a section; the matching
/// `:|×n` (a bar whose
/// [`RepeatMarker::closes`](crate::score::RepeatMarker::closes) is true) plays
/// the span `n` times in total before the timeline moves on. A close with no
/// preceding open repeats from the song start.
#[must_use]
pub fn played_bar_order(score: &Score) -> Vec<usize> {
    // Stub (red phase): the written order, ignoring repeats. The four expansion
    // tests fail against this until the real projection lands.
    let bar_count = score.master_bars.len();
    let _cap = bar_count.saturating_mul(MAX_EXPANSION);
    (0..bar_count).collect()
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::missing_assert_message
)]
mod tests {
    use super::{played_bar_order, MAX_EXPANSION};
    use crate::event::{Tempo, Ticks, TimeSignature};
    use crate::score::{LossReport, MasterBar, RepeatMarker, Score};
    use crate::slice::TickRange;

    /// A 4/4 bar at `index`, laid out back-to-back, carrying `repeat`.
    fn bar(index: usize, repeat: RepeatMarker) -> MasterBar {
        let start = u32::try_from(index).unwrap_or(0).saturating_mul(1920);
        MasterBar {
            index,
            tick_range: TickRange::new(Ticks(start), Ticks(start.saturating_add(1920)))
                .expect("ordered range"),
            time_signature: TimeSignature::new(4, 4).expect("valid meter"),
            tempo: Tempo::new(120.0).expect("valid tempo"),
            repeat,
        }
    }

    fn score(bars: Vec<MasterBar>) -> Score {
        Score {
            ticks_per_quarter: 960,
            master_bars: bars,
            tracks: Vec::new(),
            source_meta: None,
            loss: LossReport::new(),
        }
    }

    fn opens() -> RepeatMarker {
        RepeatMarker {
            start: true,
            play_count: 0,
        }
    }

    fn closes(play_count: u8) -> RepeatMarker {
        RepeatMarker {
            start: false,
            play_count,
        }
    }

    #[test]
    fn no_repeats_is_identity() {
        let s = score(vec![
            bar(0, RepeatMarker::default()),
            bar(1, RepeatMarker::default()),
            bar(2, RepeatMarker::default()),
        ]);
        assert_eq!(played_bar_order(&s), vec![0, 1, 2]);
    }

    #[test]
    fn empty_score_is_empty() {
        assert_eq!(played_bar_order(&score(Vec::new())), Vec::<usize>::new());
    }

    #[test]
    fn simple_repeat_plays_section_twice() {
        // b0  |: b1 b2 :|×2  b4  →  0,1,2,3 then the 1..=3 span once more, then 4.
        let bars = vec![
            bar(0, RepeatMarker::default()),
            bar(1, opens()),
            bar(2, RepeatMarker::default()),
            bar(3, closes(2)),
            bar(4, RepeatMarker::default()),
        ];
        assert_eq!(played_bar_order(&score(bars)), vec![0, 1, 2, 3, 1, 2, 3, 4]);
    }

    #[test]
    fn repeat_count_three_plays_three_times() {
        // |: b0 b1 :|×3  →  the span plays three times in full.
        let bars = vec![
            bar(0, opens()),
            bar(1, closes(3)),
            bar(2, RepeatMarker::default()),
        ];
        assert_eq!(played_bar_order(&score(bars)), vec![0, 1, 0, 1, 0, 1, 2]);
    }

    #[test]
    fn close_without_open_repeats_from_song_start() {
        // No `|:`; a `:|×2` at bar 2 repeats from the song start.
        let bars = vec![
            bar(0, RepeatMarker::default()),
            bar(1, RepeatMarker::default()),
            bar(2, closes(2)),
            bar(3, RepeatMarker::default()),
        ];
        assert_eq!(played_bar_order(&score(bars)), vec![0, 1, 2, 0, 1, 2, 3]);
    }

    #[test]
    fn back_to_back_sections_each_expand_independently() {
        // |: b0 :|×2   |: b2 :|×2  — two adjacent one-bar repeats.
        let bars = vec![
            bar(0, opens()),
            bar(1, closes(2)),
            bar(2, opens()),
            bar(3, closes(2)),
        ];
        assert_eq!(played_bar_order(&score(bars)), vec![0, 1, 0, 1, 2, 3, 2, 3]);
    }

    #[test]
    fn degenerate_play_count_one_is_not_a_repeat() {
        let bars = vec![bar(0, opens()), bar(1, closes(1))];
        assert_eq!(played_bar_order(&score(bars)), vec![0, 1]);
    }

    #[test]
    fn pathological_play_count_stays_bounded() {
        // A 255× repeat of a 2-bar span would be 510 entries; the cap bounds it
        // and, crucially, the call terminates.
        let bars = vec![bar(0, opens()), bar(1, closes(255))];
        let order = played_bar_order(&score(bars));
        assert!(order.len() <= bars_len_times_cap(2));
        assert_eq!(order.first().copied(), Some(0));
    }

    fn bars_len_times_cap(bar_count: usize) -> usize {
        bar_count.saturating_mul(MAX_EXPANSION)
    }
}
