//! Deterministic phrase generation primitives.

use crate::event::{
    Articulation, Bar, Event, Note, Phrase, Pitch, Tempo, Ticks, TimeSignature, ValidationError,
    Velocity,
};

/// Pattern repeated by the built-in phrase generator.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepeatingPattern {
    /// Ordered pitches to emit as notes.
    pub pitches: Vec<Pitch>,
    /// Duration of each generated note.
    pub step: Ticks,
    /// Velocity used for every generated note.
    pub velocity: Velocity,
    /// Optional articulation copied to every generated note.
    pub articulation: Option<Articulation>,
}

/// Request for deterministic phrase generation.
#[derive(Debug, Clone, PartialEq)]
pub struct GeneratePhraseRequest {
    /// Number of bars to generate.
    pub bar_count: usize,
    /// Time signature copied to each generated bar.
    pub time_signature: TimeSignature,
    /// Tempo copied to each generated bar.
    pub tempo: Tempo,
    /// PPQN resolution used to compute each bar duration.
    pub ticks_per_quarter: Ticks,
    /// Repeating note pattern.
    pub pattern: RepeatingPattern,
}

/// Computes the duration of one bar for a PPQN resolution and meter.
pub fn bar_duration_ticks(
    time_signature: TimeSignature,
    ticks_per_quarter: Ticks,
) -> Result<Ticks, ValidationError> {
    TimeSignature::new(time_signature.numerator, time_signature.denominator)?;

    if ticks_per_quarter == Ticks::ZERO {
        return Err(ValidationError::InvalidTicksPerQuarter);
    }

    let numerator_ticks = ticks_per_quarter
        .0
        .checked_mul(u32::from(time_signature.numerator))
        .ok_or(ValidationError::DurationOverflow)?;
    let whole_note_scaled = numerator_ticks
        .checked_mul(4)
        .ok_or(ValidationError::DurationOverflow)?;

    Ok(Ticks(
        whole_note_scaled
            .checked_div(u32::from(time_signature.denominator))
            .ok_or(ValidationError::InvalidTimeSignatureDenominator {
                value: time_signature.denominator,
            })?,
    ))
}

/// Generates a phrase by repeating a pitch pattern into every requested bar.
pub fn generate_repeating_phrase(
    request: &GeneratePhraseRequest,
) -> Result<Phrase, ValidationError> {
    validate_request(request)?;

    let bar_duration = bar_duration_ticks(request.time_signature, request.ticks_per_quarter)?;
    let mut bars = Vec::new();
    let mut pitch_index = 0_usize;

    for _ in 0..request.bar_count {
        let mut events = Vec::new();
        let mut cursor = Ticks::ZERO;

        while cursor < bar_duration {
            let remaining = bar_duration.checked_sub(cursor)?;
            let duration = request.pattern.step.min(remaining);
            let pitch = select_pitch(&request.pattern.pitches, pitch_index)?;

            events.push(Event::Note(Note {
                pitch,
                duration,
                velocity: request.pattern.velocity,
                articulation: request.pattern.articulation,
            }));
            cursor = cursor.checked_add(duration)?;
            pitch_index = increment_wrapping(pitch_index, request.pattern.pitches.len())?;
        }

        bars.push(Bar {
            time_signature: request.time_signature,
            tempo: request.tempo,
            events,
        });
    }

    Ok(Phrase { bars })
}

fn validate_request(request: &GeneratePhraseRequest) -> Result<(), ValidationError> {
    if request.pattern.pitches.is_empty() {
        return Err(ValidationError::EmptyPitchSequence);
    }

    if request.pattern.step == Ticks::ZERO {
        return Err(ValidationError::InvalidStepDuration);
    }

    let _bar_duration = bar_duration_ticks(request.time_signature, request.ticks_per_quarter)?;

    Ok(())
}

fn select_pitch(pitches: &[Pitch], index: usize) -> Result<Pitch, ValidationError> {
    pitches
        .get(index)
        .copied()
        .ok_or(ValidationError::EmptyPitchSequence)
}

fn increment_wrapping(value: usize, len: usize) -> Result<usize, ValidationError> {
    let next = value.checked_add(1).ok_or(ValidationError::CountOverflow)?;
    if next == len {
        Ok(0)
    } else {
        Ok(next)
    }
}

#[cfg(test)]
mod tests {
    use super::{
        bar_duration_ticks, generate_repeating_phrase, GeneratePhraseRequest, RepeatingPattern,
    };
    use crate::event::{
        Articulation, Bar, Event, Note, Phrase, Pitch, Tempo, Ticks, TimeSignature,
        ValidationError, Velocity,
    };

    fn request() -> GeneratePhraseRequest {
        GeneratePhraseRequest {
            bar_count: 2,
            time_signature: TimeSignature {
                numerator: 4,
                denominator: 4,
            },
            tempo: Tempo(120.0),
            ticks_per_quarter: Ticks(480),
            pattern: RepeatingPattern {
                pitches: vec![Pitch(60), Pitch(64), Pitch(67)],
                step: Ticks(480),
                velocity: Velocity(96),
                articulation: Some(Articulation::PalmMute),
            },
        }
    }

    fn meter() -> TimeSignature {
        TimeSignature {
            numerator: 4,
            denominator: 4,
        }
    }

    fn note(pitch: u8, duration: u32) -> Event {
        Event::Note(Note {
            pitch: Pitch(pitch),
            duration: Ticks(duration),
            velocity: Velocity(96),
            articulation: Some(Articulation::PalmMute),
        })
    }

    fn generated_bar(events: Vec<Event>) -> Bar {
        Bar {
            time_signature: meter(),
            tempo: Tempo(120.0),
            events,
        }
    }

    #[test]
    fn bar_duration_ticks_uses_meter_and_ppqn() {
        assert_eq!(
            bar_duration_ticks(
                TimeSignature {
                    numerator: 4,
                    denominator: 4,
                },
                Ticks(480),
            ),
            Ok(Ticks(1920)),
        );
        assert_eq!(
            bar_duration_ticks(
                TimeSignature {
                    numerator: 7,
                    denominator: 8,
                },
                Ticks(480),
            ),
            Ok(Ticks(1680)),
        );
    }

    #[test]
    fn bar_duration_ticks_rejects_invalid_inputs() {
        assert_eq!(
            bar_duration_ticks(
                TimeSignature {
                    numerator: 4,
                    denominator: 4,
                },
                Ticks(0),
            ),
            Err(ValidationError::InvalidTicksPerQuarter),
        );
        assert_eq!(
            bar_duration_ticks(
                TimeSignature {
                    numerator: 4,
                    denominator: 3,
                },
                Ticks(480),
            ),
            Err(ValidationError::InvalidTimeSignatureDenominator { value: 3 }),
        );
    }

    #[test]
    fn generate_repeating_phrase_fills_requested_bars() {
        assert_eq!(
            generate_repeating_phrase(&request()),
            Ok(Phrase {
                bars: vec![
                    generated_bar(vec![
                        note(60, 480),
                        note(64, 480),
                        note(67, 480),
                        note(60, 480),
                    ]),
                    generated_bar(vec![
                        note(64, 480),
                        note(67, 480),
                        note(60, 480),
                        note(64, 480),
                    ]),
                ],
            }),
        );
    }

    #[test]
    fn generate_repeating_phrase_truncates_last_step_to_bar() {
        let mut source = request();
        source.bar_count = 1;
        source.pattern.step = Ticks(700);

        assert_eq!(
            generate_repeating_phrase(&source),
            Ok(Phrase {
                bars: vec![generated_bar(vec![
                    note(60, 700),
                    note(64, 700),
                    note(67, 520),
                ])],
            }),
        );
    }

    #[test]
    fn generate_repeating_phrase_rejects_empty_pitch_sequence() {
        let mut source = request();
        source.pattern.pitches = Vec::new();

        assert_eq!(
            generate_repeating_phrase(&source),
            Err(ValidationError::EmptyPitchSequence),
        );
    }

    #[test]
    fn generate_repeating_phrase_rejects_zero_step() {
        let mut source = request();
        source.pattern.step = Ticks::ZERO;

        assert_eq!(
            generate_repeating_phrase(&source),
            Err(ValidationError::InvalidStepDuration),
        );
    }
}
