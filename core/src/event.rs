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
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
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

#[cfg(test)]
mod tests {
    use super::{Pitch, Tempo, Ticks, TimeSignature, ValidationError, Velocity};

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
}
