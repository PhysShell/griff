//! Half-open tick-range primitive and bar-range extraction over the canonical
//! model.

use std::ops::Range;

use crate::event::{Ticks, ValidationError};
use crate::score::{
    AtomEvent, AtomNote, AtomRest, EventGroup, LossReport, MasterBar, Score, TechniqueSpan, Track,
    Voice,
};

/// Half-open tick range: `start <= tick < end`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TickRange {
    /// Inclusive range start.
    pub start: Ticks,
    /// Exclusive range end.
    pub end: Ticks,
}

impl TickRange {
    /// Creates a half-open range after checking `start <= end`.
    pub const fn new(start: Ticks, end: Ticks) -> Result<Self, ValidationError> {
        if start.0 <= end.0 {
            Ok(Self { start, end })
        } else {
            Err(ValidationError::InvalidTickRange)
        }
    }

    /// Range length in ticks.
    pub fn len(self) -> Result<Ticks, ValidationError> {
        self.end.checked_sub(self.start)
    }

    /// Returns whether this range contains no ticks.
    pub const fn is_empty(self) -> bool {
        self.start.0 == self.end.0
    }

    /// Returns whether an event starting at `event_start` with `duration` intersects the range.
    pub fn intersects_event(
        self,
        event_start: Ticks,
        duration: Ticks,
    ) -> Result<bool, ValidationError> {
        let event_end = event_start.checked_add(duration)?;
        Ok(event_start.0 < self.end.0 && event_end.0 > self.start.0)
    }
}

/// Extracts a contiguous run of bars `bars` as a standalone [`Score`].
///
/// The selected bars are re-indexed from 0 and every tick — bars, notes, rests,
/// technique spans — is rebased so the slice starts at tick 0; notes and rests
/// outside the span are dropped by onset, and spans are clamped to it. `bars.end`
/// is clamped to the bar count, and an empty or reversed range yields a score
/// with no bars (tracks and voices are preserved, their event groups empty). The
/// result is independently measurable — the cut `griff split` makes per phrase.
#[must_use]
pub fn extract_bars(score: &Score, bars: Range<usize>) -> Score {
    let count = score.master_bars.len();
    let lo = bars.start.min(count);
    let hi = bars.end.clamp(lo, count);
    let selected = score.master_bars.get(lo..hi).unwrap_or_default();

    let offset = selected.first().map_or(0, |b| b.tick_range.start.0);
    let seg_end = selected.last().map_or(offset, |b| b.tick_range.end.0);

    let master_bars = selected
        .iter()
        .enumerate()
        .map(|(i, b)| MasterBar {
            index: i,
            tick_range: rebased_range(b.tick_range, offset),
            time_signature: b.time_signature,
            tempo: b.tempo,
            repeat: b.repeat,
        })
        .collect();

    let tracks = score
        .tracks
        .iter()
        .map(|t| Track {
            name: t.name.clone(),
            channel: t.channel,
            tuning: t.tuning.clone(),
            voices: t
                .voices
                .iter()
                .map(|v| Voice {
                    id: v.id,
                    event_groups: v
                        .event_groups
                        .iter()
                        .filter_map(|g| sliced_group(g, offset, seg_end))
                        .collect(),
                })
                .collect(),
        })
        .collect();

    Score {
        ticks_per_quarter: score.ticks_per_quarter,
        master_bars,
        tracks,
        source_meta: score.source_meta.clone(),
        loss: LossReport::new(),
    }
}

/// Shifts a range down by `offset`, saturating at zero.
const fn rebased_range(range: TickRange, offset: u32) -> TickRange {
    TickRange {
        start: Ticks(range.start.0.saturating_sub(offset)),
        end: Ticks(range.end.0.saturating_sub(offset)),
    }
}

/// Keeps a group's atoms whose onset falls in `[seg_start, seg_end)` and the
/// spans that overlap it, all rebased to the slice. `None` when nothing remains.
fn sliced_group(group: &EventGroup, seg_start: u32, seg_end: u32) -> Option<EventGroup> {
    let atoms: Vec<AtomEvent> = group
        .atoms
        .iter()
        .filter(|a| {
            let onset = a.absolute_start().0;
            onset >= seg_start && onset < seg_end
        })
        .map(|a| rebased_atom(*a, seg_start))
        .collect();
    if atoms.is_empty() {
        return None;
    }
    let technique_spans = group
        .technique_spans
        .iter()
        .filter(|s| s.tick_range.start.0 < seg_end && s.tick_range.end.0 > seg_start)
        .map(|s| TechniqueSpan {
            technique: s.technique,
            tick_range: rebased_range(
                TickRange {
                    start: Ticks(s.tick_range.start.0.max(seg_start)),
                    end: Ticks(s.tick_range.end.0.min(seg_end)),
                },
                seg_start,
            ),
            evidence: s.evidence,
        })
        .collect();
    Some(EventGroup {
        kind: group.kind,
        atoms,
        technique_spans,
    })
}

/// Rebases a single atom's onset down by `offset`, saturating at zero.
const fn rebased_atom(atom: AtomEvent, offset: u32) -> AtomEvent {
    match atom {
        AtomEvent::Note(n) => AtomEvent::Note(AtomNote {
            absolute_start: Ticks(n.absolute_start.0.saturating_sub(offset)),
            ..n
        }),
        AtomEvent::Rest(r) => AtomEvent::Rest(AtomRest {
            absolute_start: Ticks(r.absolute_start.0.saturating_sub(offset)),
            ..r
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::TickRange;
    use crate::event::{Ticks, ValidationError};

    #[test]
    fn tick_range_accepts_ordered_bounds() {
        assert_eq!(
            TickRange::new(Ticks(10), Ticks(20)),
            Ok(TickRange {
                start: Ticks(10),
                end: Ticks(20),
            }),
        );
    }

    #[test]
    fn tick_range_rejects_reversed_bounds() {
        assert_eq!(
            TickRange::new(Ticks(20), Ticks(10)),
            Err(ValidationError::InvalidTickRange),
        );
    }

    #[test]
    fn tick_range_len_and_empty_follow_half_open_bounds() {
        let range = TickRange {
            start: Ticks(10),
            end: Ticks(20),
        };
        assert_eq!(range.len(), Ok(Ticks(10)));
        assert!(!range.is_empty(), "non-empty range must report false");

        let empty = TickRange {
            start: Ticks(10),
            end: Ticks(10),
        };
        assert_eq!(empty.len(), Ok(Ticks(0)));
        assert!(empty.is_empty(), "empty range must report true");
    }

    #[test]
    fn tick_range_detects_event_intersection() {
        let range = TickRange {
            start: Ticks(100),
            end: Ticks(200),
        };

        assert_eq!(range.intersects_event(Ticks(0), Ticks(100)), Ok(false));
        assert_eq!(range.intersects_event(Ticks(0), Ticks(101)), Ok(true));
        assert_eq!(range.intersects_event(Ticks(150), Ticks(10)), Ok(true));
        assert_eq!(range.intersects_event(Ticks(200), Ticks(10)), Ok(false));
    }
}
