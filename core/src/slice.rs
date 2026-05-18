//! Tick-range slicing helpers for phrases and bars.

use crate::event::{Bar, Event, Phrase, Ticks, ValidationError};

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

/// Event annotated with absolute phrase position and source indexes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimedEvent {
    /// Zero-based bar index in the source phrase.
    pub bar_index: usize,
    /// Zero-based event index in the source bar.
    pub event_index: usize,
    /// Absolute event start in phrase ticks.
    pub absolute_start: Ticks,
    /// Source event payload.
    pub event: Event,
}

/// Returns all events in a bar annotated with absolute starts.
pub fn timed_bar_events(
    bar: &Bar,
    bar_index: usize,
    absolute_bar_start: Ticks,
) -> Result<Vec<TimedEvent>, ValidationError> {
    let mut cursor = absolute_bar_start;
    let mut events = Vec::new();

    for (event_index, event) in bar.events.iter().copied().enumerate() {
        events.push(TimedEvent {
            bar_index,
            event_index,
            absolute_start: cursor,
            event,
        });
        cursor = cursor.checked_add(event.duration())?;
    }

    Ok(events)
}

/// Returns all events in a phrase annotated with absolute starts.
pub fn timed_phrase_events(phrase: &Phrase) -> Result<Vec<TimedEvent>, ValidationError> {
    let mut bar_start = Ticks::ZERO;
    let mut events = Vec::new();

    for (bar_index, bar) in phrase.bars.iter().enumerate() {
        events.extend(timed_bar_events(bar, bar_index, bar_start)?);
        bar_start = bar_start.checked_add(bar.duration()?)?;
    }

    Ok(events)
}

/// Returns all phrase events whose half-open spans intersect `range`.
pub fn slice_phrase_events(
    phrase: &Phrase,
    range: TickRange,
) -> Result<Vec<TimedEvent>, ValidationError> {
    let mut events = Vec::new();

    for timed_event in timed_phrase_events(phrase)? {
        if range.intersects_event(timed_event.absolute_start, timed_event.event.duration())? {
            events.push(timed_event);
        }
    }

    Ok(events)
}

#[cfg(test)]
mod tests {
    use super::{
        slice_phrase_events, timed_bar_events, timed_phrase_events, TickRange, TimedEvent,
    };
    use crate::event::{Bar, Event, Phrase, Rest, Tempo, Ticks, TimeSignature, ValidationError};

    fn rest(duration: u32) -> Event {
        Event::Rest(Rest {
            duration: Ticks(duration),
        })
    }

    fn bar(durations: &[u32]) -> Bar {
        Bar {
            time_signature: TimeSignature {
                numerator: 4,
                denominator: 4,
            },
            tempo: Tempo(120.0),
            events: durations.iter().copied().map(rest).collect(),
        }
    }

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

    #[test]
    fn timed_bar_events_emit_absolute_starts_and_indexes() {
        let source = bar(&[120, 240, 360]);
        assert_eq!(
            timed_bar_events(&source, 2, Ticks(480)),
            Ok(vec![
                TimedEvent {
                    bar_index: 2,
                    event_index: 0,
                    absolute_start: Ticks(480),
                    event: rest(120),
                },
                TimedEvent {
                    bar_index: 2,
                    event_index: 1,
                    absolute_start: Ticks(600),
                    event: rest(240),
                },
                TimedEvent {
                    bar_index: 2,
                    event_index: 2,
                    absolute_start: Ticks(840),
                    event: rest(360),
                },
            ]),
        );
    }

    #[test]
    fn timed_phrase_events_cross_bar_boundaries() {
        let phrase = Phrase {
            bars: vec![bar(&[120, 360]), bar(&[240])],
        };

        assert_eq!(
            timed_phrase_events(&phrase),
            Ok(vec![
                TimedEvent {
                    bar_index: 0,
                    event_index: 0,
                    absolute_start: Ticks(0),
                    event: rest(120),
                },
                TimedEvent {
                    bar_index: 0,
                    event_index: 1,
                    absolute_start: Ticks(120),
                    event: rest(360),
                },
                TimedEvent {
                    bar_index: 1,
                    event_index: 0,
                    absolute_start: Ticks(480),
                    event: rest(240),
                },
            ]),
        );
    }

    #[test]
    fn slice_phrase_events_returns_intersecting_events() {
        let phrase = Phrase {
            bars: vec![bar(&[120, 360]), bar(&[240, 240])],
        };
        let range = TickRange {
            start: Ticks(100),
            end: Ticks(500),
        };

        assert_eq!(
            slice_phrase_events(&phrase, range),
            Ok(vec![
                TimedEvent {
                    bar_index: 0,
                    event_index: 0,
                    absolute_start: Ticks(0),
                    event: rest(120),
                },
                TimedEvent {
                    bar_index: 0,
                    event_index: 1,
                    absolute_start: Ticks(120),
                    event: rest(360),
                },
                TimedEvent {
                    bar_index: 1,
                    event_index: 0,
                    absolute_start: Ticks(480),
                    event: rest(240),
                },
            ]),
        );
    }

    #[test]
    fn slice_phrase_events_excludes_events_on_half_open_boundaries() {
        let phrase = Phrase {
            bars: vec![bar(&[100, 100, 100])],
        };
        let range = TickRange {
            start: Ticks(100),
            end: Ticks(200),
        };

        assert_eq!(
            slice_phrase_events(&phrase, range),
            Ok(vec![TimedEvent {
                bar_index: 0,
                event_index: 1,
                absolute_start: Ticks(100),
                event: rest(100),
            }]),
        );
    }

    #[test]
    fn timed_phrase_events_reports_duration_overflow() {
        let phrase = Phrase {
            bars: vec![bar(&[u32::MAX]), bar(&[1])],
        };

        assert_eq!(
            timed_phrase_events(&phrase),
            Err(ValidationError::DurationOverflow),
        );
    }
}
