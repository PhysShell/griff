//! Feature extraction over structured musical phrases.

use crate::event::{Event, Phrase, Pitch, Ticks, ValidationError, Velocity};

/// Inclusive pitch span found in a phrase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PitchRange {
    /// Lowest MIDI pitch in the phrase.
    pub lowest: Pitch,
    /// Highest MIDI pitch in the phrase.
    pub highest: Pitch,
}

impl PitchRange {
    fn include(self, pitch: Pitch) -> Self {
        Self {
            lowest: self.lowest.min(pitch),
            highest: self.highest.max(pitch),
        }
    }
}

/// Inclusive velocity span found in a phrase.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VelocityRange {
    /// Lowest MIDI velocity in the phrase.
    pub lowest: Velocity,
    /// Highest MIDI velocity in the phrase.
    pub highest: Velocity,
}

impl VelocityRange {
    fn include(self, velocity: Velocity) -> Self {
        Self {
            lowest: self.lowest.min(velocity),
            highest: self.highest.max(velocity),
        }
    }
}

/// Basic phrase-level features useful for scheduling, generation, and analysis.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PhraseFeatures {
    /// Number of bars in the phrase.
    pub bar_count: usize,
    /// Number of events across all bars.
    pub event_count: usize,
    /// Number of note events.
    pub note_count: usize,
    /// Number of rest events.
    pub rest_count: usize,
    /// Number of notes carrying articulation metadata.
    pub articulated_note_count: usize,
    /// Total phrase duration in ticks.
    pub total_duration: Ticks,
    /// Pitch span across notes, or `None` when the phrase has no notes.
    pub pitch_range: Option<PitchRange>,
    /// Velocity span across notes, or `None` when the phrase has no notes.
    pub velocity_range: Option<VelocityRange>,
}

/// Extracts basic phrase-level counts, spans, and total duration.
pub fn phrase_features(phrase: &Phrase) -> Result<PhraseFeatures, ValidationError> {
    let mut features = PhraseFeatures {
        bar_count: phrase.bars.len(),
        event_count: 0,
        note_count: 0,
        rest_count: 0,
        articulated_note_count: 0,
        total_duration: Ticks::ZERO,
        pitch_range: None,
        velocity_range: None,
    };

    for bar in &phrase.bars {
        for event in &bar.events {
            features.event_count = increment(features.event_count)?;
            features.total_duration = features.total_duration.checked_add(event.duration())?;

            match event {
                Event::Note(note) => {
                    features.note_count = increment(features.note_count)?;
                    if note.articulation.is_some() {
                        features.articulated_note_count =
                            increment(features.articulated_note_count)?;
                    }
                    features.pitch_range = Some(features.pitch_range.map_or(
                        PitchRange {
                            lowest: note.pitch,
                            highest: note.pitch,
                        },
                        |range| range.include(note.pitch),
                    ));
                    features.velocity_range = Some(features.velocity_range.map_or(
                        VelocityRange {
                            lowest: note.velocity,
                            highest: note.velocity,
                        },
                        |range| range.include(note.velocity),
                    ));
                }
                Event::Rest(_) => {
                    features.rest_count = increment(features.rest_count)?;
                }
            }
        }
    }

    Ok(features)
}

fn increment(value: usize) -> Result<usize, ValidationError> {
    value.checked_add(1).ok_or(ValidationError::CountOverflow)
}

#[cfg(test)]
mod tests {
    use super::{phrase_features, PhraseFeatures, PitchRange, VelocityRange};
    use crate::event::{
        Articulation, Bar, Event, Note, Phrase, Pitch, Rest, Tempo, Ticks, TimeSignature,
        ValidationError, Velocity,
    };

    fn meter() -> TimeSignature {
        TimeSignature {
            numerator: 4,
            denominator: 4,
        }
    }

    fn note(pitch: u8, duration: u32, velocity: u8, articulation: Option<Articulation>) -> Event {
        Event::Note(Note {
            pitch: Pitch(pitch),
            duration: Ticks(duration),
            velocity: Velocity(velocity),
            articulation,
        })
    }

    fn rest(duration: u32) -> Event {
        Event::Rest(Rest {
            duration: Ticks(duration),
        })
    }

    #[test]
    fn phrase_features_extracts_counts_spans_and_duration() {
        let phrase = Phrase {
            bars: vec![
                Bar {
                    time_signature: meter(),
                    tempo: Tempo(120.0),
                    events: vec![note(64, 120, 80, None), rest(60)],
                },
                Bar {
                    time_signature: meter(),
                    tempo: Tempo(120.0),
                    events: vec![
                        note(60, 240, 100, Some(Articulation::PalmMute)),
                        note(67, 120, 70, None),
                    ],
                },
            ],
        };

        assert_eq!(
            phrase_features(&phrase),
            Ok(PhraseFeatures {
                bar_count: 2,
                event_count: 4,
                note_count: 3,
                rest_count: 1,
                articulated_note_count: 1,
                total_duration: Ticks(540),
                pitch_range: Some(PitchRange {
                    lowest: Pitch(60),
                    highest: Pitch(67),
                }),
                velocity_range: Some(VelocityRange {
                    lowest: Velocity(70),
                    highest: Velocity(100),
                }),
            }),
        );
    }

    #[test]
    fn phrase_features_reports_empty_phrase() {
        let phrase = Phrase { bars: Vec::new() };

        assert_eq!(
            phrase_features(&phrase),
            Ok(PhraseFeatures {
                bar_count: 0,
                event_count: 0,
                note_count: 0,
                rest_count: 0,
                articulated_note_count: 0,
                total_duration: Ticks(0),
                pitch_range: None,
                velocity_range: None,
            }),
        );
    }

    #[test]
    fn phrase_features_keeps_spans_empty_for_rest_only_phrase() {
        let phrase = Phrase {
            bars: vec![Bar {
                time_signature: meter(),
                tempo: Tempo(120.0),
                events: vec![rest(120), rest(360)],
            }],
        };

        assert_eq!(
            phrase_features(&phrase),
            Ok(PhraseFeatures {
                bar_count: 1,
                event_count: 2,
                note_count: 0,
                rest_count: 2,
                articulated_note_count: 0,
                total_duration: Ticks(480),
                pitch_range: None,
                velocity_range: None,
            }),
        );
    }

    #[test]
    fn phrase_features_reports_duration_overflow() {
        let phrase = Phrase {
            bars: vec![Bar {
                time_signature: meter(),
                tempo: Tempo(120.0),
                events: vec![rest(u32::MAX), rest(1)],
            }],
        };

        assert_eq!(
            phrase_features(&phrase),
            Err(ValidationError::DurationOverflow),
        );
    }
}
