//! Half-open tick-range primitive shared across the canonical model.

use crate::event::{Ticks, ValidationError};

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
