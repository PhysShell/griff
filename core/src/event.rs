//! Fundamental musical event types.

/// Maximum value accepted by MIDI 7-bit fields.
pub const MIDI_7_BIT_MAX: u8 = 127;

/// Validation error for core musical values.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValidationError {
    /// MIDI pitch must be in the inclusive `0..=127` range.
    PitchOutOfRange { value: u8 },
    /// MIDI velocity must be in the inclusive `0..=127` range.
    VelocityOutOfRange { value: u8 },
    /// Tempo must be finite and greater than zero beats per minute.
    InvalidTempo,
    /// Time-signature numerator must be greater than zero.
    InvalidTimeSignatureNumerator,
    /// Time-signature denominator must be a non-zero power of two.
    InvalidTimeSignatureDenominator { value: u8 },
    /// Summing event durations exceeded the `Ticks` storage capacity.
    DurationOverflow,
    /// Tick ranges must be ordered as `start <= end`.
    InvalidTickRange,
    /// Counting musical items exceeded the platform `usize` capacity.
    CountOverflow,
    /// Generation requires at least one pitch to repeat.
    EmptyPitchSequence,
    /// PPQN resolution must be greater than zero.
    InvalidTicksPerQuarter,
    /// Generated note step duration must be greater than zero.
    InvalidStepDuration,
}

/// MIDI pitch number (0–127; 60 = middle C).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Pitch(pub u8);

impl Pitch {
    /// Creates a pitch after checking the MIDI 7-bit range.
    pub const fn new(value: u8) -> Result<Self, ValidationError> {
        if value <= MIDI_7_BIT_MAX {
            Ok(Self(value))
        } else {
            Err(ValidationError::PitchOutOfRange { value })
        }
    }
}

/// Duration in ticks (PPQN-relative; track resolution is carried externally).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Ticks(pub u32);

impl Ticks {
    /// Zero ticks.
    pub const ZERO: Self = Self(0);

    /// Adds two tick durations, returning an error on overflow.
    pub fn checked_add(self, rhs: Self) -> Result<Self, ValidationError> {
        self.0
            .checked_add(rhs.0)
            .map(Self)
            .ok_or(ValidationError::DurationOverflow)
    }

    /// Subtracts two tick durations, returning an error on underflow.
    pub fn checked_sub(self, rhs: Self) -> Result<Self, ValidationError> {
        self.0
            .checked_sub(rhs.0)
            .map(Self)
            .ok_or(ValidationError::InvalidTickRange)
    }
}

/// MIDI velocity (0–127).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Velocity(pub u8);

impl Velocity {
    /// Creates a velocity after checking the MIDI 7-bit range.
    pub const fn new(value: u8) -> Result<Self, ValidationError> {
        if value <= MIDI_7_BIT_MAX {
            Ok(Self(value))
        } else {
            Err(ValidationError::VelocityOutOfRange { value })
        }
    }
}

/// Tempo in beats per minute.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct Tempo(pub f64);

impl Tempo {
    /// Creates a tempo after checking that it is finite and positive.
    pub fn new(beats_per_minute: f64) -> Result<Self, ValidationError> {
        if beats_per_minute.is_finite() && beats_per_minute > 0.0 {
            Ok(Self(beats_per_minute))
        } else {
            Err(ValidationError::InvalidTempo)
        }
    }
}

/// Time signature, e.g. 4/4 or 7/8.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TimeSignature {
    /// Beats per measure.
    pub numerator: u8,
    /// Beat unit as a power of two (2 = half-note, 4 = quarter-note, …).
    pub denominator: u8,
}

impl TimeSignature {
    /// Creates a time signature with a non-zero numerator and power-of-two denominator.
    pub const fn new(numerator: u8, denominator: u8) -> Result<Self, ValidationError> {
        if numerator == 0 {
            Err(ValidationError::InvalidTimeSignatureNumerator)
        } else if !denominator.is_power_of_two() {
            Err(ValidationError::InvalidTimeSignatureDenominator { value: denominator })
        } else {
            Ok(Self {
                numerator,
                denominator,
            })
        }
    }
}

/// Per-note guitar articulation carried as optional metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Articulation {
    /// Slide into or out of the note.
    Slide,
    /// Pitch bend up or down.
    Bend,
    /// Legato (slur).
    Legato,
    /// Palm mute.
    PalmMute,
    /// Hammer-on.
    HammerOn,
    /// Pull-off.
    PullOff,
    /// Vibrato.
    Vibrato,
    /// Natural harmonic.
    HarmonicNatural,
    /// Pinch harmonic.
    HarmonicPinch,
}

/// A sounding note.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Note {
    /// MIDI pitch.
    pub pitch: Pitch,
    /// Duration in ticks.
    pub duration: Ticks,
    /// MIDI velocity.
    pub velocity: Velocity,
    /// Optional playing technique.
    pub articulation: Option<Articulation>,
}

/// A silence of a given duration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rest {
    /// Duration in ticks.
    pub duration: Ticks,
}

/// A musical event: a sounding note or a silence.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Event {
    /// A sounding note.
    Note(Note),
    /// A silence.
    Rest(Rest),
}

impl Event {
    /// Duration of this event regardless of its kind.
    pub const fn duration(self) -> Ticks {
        match self {
            Self::Note(n) => n.duration,
            Self::Rest(r) => r.duration,
        }
    }
}

/// One measure: its events plus the governing meter and tempo.
#[derive(Debug, Clone, PartialEq)]
pub struct Bar {
    /// Meter of this bar.
    pub time_signature: TimeSignature,
    /// Tempo at bar start.
    pub tempo: Tempo,
    /// Ordered events that fill the bar.
    pub events: Vec<Event>,
}

impl Bar {
    /// Total duration of all events in the bar.
    pub fn duration(&self) -> Result<Ticks, ValidationError> {
        self.events.iter().try_fold(Ticks::ZERO, |total, event| {
            total.checked_add(event.duration())
        })
    }
}

/// An ordered sequence of bars forming a musical phrase.
#[derive(Debug, Clone, PartialEq)]
pub struct Phrase {
    /// Bars in order.
    pub bars: Vec<Bar>,
}

impl Phrase {
    /// Total duration of all bars in the phrase.
    pub fn duration(&self) -> Result<Ticks, ValidationError> {
        self.bars
            .iter()
            .try_fold(Ticks::ZERO, |total, bar| total.checked_add(bar.duration()?))
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Articulation, Bar, Event, Note, Phrase, Pitch, Rest, Tempo, Ticks, TimeSignature,
        ValidationError, Velocity,
    };

    #[test]
    fn pitch_accepts_midi_range() {
        assert_eq!(Pitch::new(0), Ok(Pitch(0)));
        assert_eq!(Pitch::new(127), Ok(Pitch(127)));
    }

    #[test]
    fn pitch_rejects_values_outside_midi_range() {
        assert_eq!(
            Pitch::new(128),
            Err(ValidationError::PitchOutOfRange { value: 128 }),
        );
    }

    #[test]
    fn velocity_accepts_midi_range() {
        assert_eq!(Velocity::new(0), Ok(Velocity(0)));
        assert_eq!(Velocity::new(127), Ok(Velocity(127)));
    }

    #[test]
    fn velocity_rejects_values_outside_midi_range() {
        assert_eq!(
            Velocity::new(128),
            Err(ValidationError::VelocityOutOfRange { value: 128 }),
        );
    }

    #[test]
    fn tempo_accepts_positive_finite_values() {
        assert_eq!(Tempo::new(120.0), Ok(Tempo(120.0)));
    }

    #[test]
    fn tempo_rejects_invalid_values() {
        assert_eq!(Tempo::new(0.0), Err(ValidationError::InvalidTempo));
        assert_eq!(
            Tempo::new(f64::INFINITY),
            Err(ValidationError::InvalidTempo)
        );
    }

    #[test]
    fn time_signature_accepts_common_meter() {
        assert_eq!(
            TimeSignature::new(4, 4),
            Ok(TimeSignature {
                numerator: 4,
                denominator: 4
            }),
        );
    }

    #[test]
    fn time_signature_rejects_invalid_components() {
        assert_eq!(
            TimeSignature::new(0, 4),
            Err(ValidationError::InvalidTimeSignatureNumerator),
        );
        assert_eq!(
            TimeSignature::new(4, 3),
            Err(ValidationError::InvalidTimeSignatureDenominator { value: 3 }),
        );
    }

    #[test]
    fn ticks_checked_add_reports_overflow() {
        assert_eq!(Ticks(1).checked_add(Ticks(2)), Ok(Ticks(3)));
        assert_eq!(
            Ticks(u32::MAX).checked_add(Ticks(1)),
            Err(ValidationError::DurationOverflow),
        );
    }

    #[test]
    fn ticks_checked_sub_reports_underflow() {
        assert_eq!(Ticks(3).checked_sub(Ticks(1)), Ok(Ticks(2)));
        assert_eq!(
            Ticks(1).checked_sub(Ticks(3)),
            Err(ValidationError::InvalidTickRange),
        );
    }

    #[test]
    fn note_event_duration_matches() {
        let note = Note {
            pitch: Pitch(60),
            duration: Ticks(480),
            velocity: Velocity(100),
            articulation: None,
        };
        assert_eq!(
            Event::Note(note).duration(),
            Ticks(480),
            "note event duration must equal the inner note duration",
        );
    }

    #[test]
    fn rest_event_duration_matches() {
        let rest = Rest {
            duration: Ticks(240),
        };
        assert_eq!(
            Event::Rest(rest).duration(),
            Ticks(240),
            "rest event duration must equal the inner rest duration",
        );
    }

    #[test]
    fn bar_holds_events() {
        let note = Note {
            pitch: Pitch(64),
            duration: Ticks(480),
            velocity: Velocity(80),
            articulation: Some(Articulation::PalmMute),
        };
        let bar = Bar {
            time_signature: TimeSignature {
                numerator: 4,
                denominator: 4,
            },
            tempo: Tempo(120.0),
            events: vec![Event::Note(note)],
        };
        assert_eq!(
            bar.events.len(),
            1,
            "bar must contain the one event that was added",
        );
    }

    #[test]
    fn bar_duration_sums_events() {
        let bar = Bar {
            time_signature: TimeSignature {
                numerator: 4,
                denominator: 4,
            },
            tempo: Tempo(120.0),
            events: vec![
                Event::Rest(Rest {
                    duration: Ticks(120),
                }),
                Event::Rest(Rest {
                    duration: Ticks(360),
                }),
            ],
        };
        assert_eq!(bar.duration(), Ok(Ticks(480)));
    }

    #[test]
    fn bar_duration_reports_overflow() {
        let bar = Bar {
            time_signature: TimeSignature {
                numerator: 4,
                denominator: 4,
            },
            tempo: Tempo(120.0),
            events: vec![
                Event::Rest(Rest {
                    duration: Ticks(u32::MAX),
                }),
                Event::Rest(Rest { duration: Ticks(1) }),
            ],
        };
        assert_eq!(bar.duration(), Err(ValidationError::DurationOverflow));
    }

    #[test]
    fn phrase_collects_bars() {
        let bar = Bar {
            time_signature: TimeSignature {
                numerator: 7,
                denominator: 8,
            },
            tempo: Tempo(140.0),
            events: vec![Event::Rest(Rest {
                duration: Ticks(1920),
            })],
        };
        let phrase = Phrase { bars: vec![bar] };
        assert_eq!(
            phrase.bars.len(),
            1,
            "phrase must contain the one bar that was added",
        );
    }

    #[test]
    fn phrase_duration_sums_bars() {
        let phrase = Phrase {
            bars: vec![
                Bar {
                    time_signature: TimeSignature {
                        numerator: 4,
                        denominator: 4,
                    },
                    tempo: Tempo(120.0),
                    events: vec![Event::Rest(Rest {
                        duration: Ticks(480),
                    })],
                },
                Bar {
                    time_signature: TimeSignature {
                        numerator: 4,
                        denominator: 4,
                    },
                    tempo: Tempo(120.0),
                    events: vec![Event::Rest(Rest {
                        duration: Ticks(960),
                    })],
                },
            ],
        };
        assert_eq!(phrase.duration(), Ok(Ticks(1440)));
    }
}
