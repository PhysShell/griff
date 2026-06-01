//! Feature extraction over the canonical model.
//!
//! [`voice_features`] extracts basic counts, spans, and total duration from a
//! [`Voice`].

use crate::event::{Pitch, Ticks, ValidationError, Velocity};
use crate::score::{AtomEvent, Voice};

/// Inclusive pitch span found in a voice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PitchRange {
    /// Lowest MIDI pitch in the voice.
    pub lowest: Pitch,
    /// Highest MIDI pitch in the voice.
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

/// Inclusive velocity span found in a voice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VelocityRange {
    /// Lowest MIDI velocity in the voice.
    pub lowest: Velocity,
    /// Highest MIDI velocity in the voice.
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

/// Voice-level features over the canonical model.
///
/// Bars are a score-level [`MasterBar`](crate::score::MasterBar) concept, not a
/// property of a [`Voice`], so there is no `bar_count`. Counts span every atom
/// of every event group, so chord and arpeggio atoms each contribute.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VoiceFeatures {
    /// Number of atom events across all groups.
    pub event_count: usize,
    /// Number of note atoms.
    pub note_count: usize,
    /// Number of rest atoms.
    pub rest_count: usize,
    /// Number of note atoms carrying articulation metadata.
    pub articulated_note_count: usize,
    /// Summed duration of all atoms in ticks.
    pub total_duration: Ticks,
    /// Pitch span across note atoms, or `None` when the voice has no notes.
    pub pitch_range: Option<PitchRange>,
    /// Velocity span across note atoms, or `None` when the voice has no notes.
    pub velocity_range: Option<VelocityRange>,
}

/// Extracts voice-level counts, spans, and total duration over the canonical
/// model.
pub fn voice_features(voice: &Voice) -> Result<VoiceFeatures, ValidationError> {
    let mut features = VoiceFeatures {
        event_count: 0,
        note_count: 0,
        rest_count: 0,
        articulated_note_count: 0,
        total_duration: Ticks::ZERO,
        pitch_range: None,
        velocity_range: None,
    };

    for group in &voice.event_groups {
        for atom in &group.atoms {
            features.event_count = increment(features.event_count)?;
            features.total_duration = features.total_duration.checked_add(atom.duration())?;

            match atom {
                AtomEvent::Note(note) => {
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
                AtomEvent::Rest(_) => {
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
