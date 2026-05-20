//! Piano-roll view over MIDI bytes (S8).

use griff_core::{
    event::Event,
    midi::{self, MidiError},
};
use thiserror::Error;

// ── error ─────────────────────────────────────────────────────────────────────

/// Error produced when constructing a [`PianoRollView`].
#[derive(Debug, Error)]
pub enum PianoRollError {
    /// The MIDI bytes could not be parsed or contain no tempo.
    #[error("MIDI error: {0}")]
    Midi(#[from] MidiError),
    /// The file contained no note events.
    #[error("MIDI file contains no notes")]
    Empty,
}

// ── data types ────────────────────────────────────────────────────────────────

/// A single note in the piano-roll grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PianoRollNote {
    /// MIDI pitch (0–127).
    pub pitch: u8,
    /// Absolute start tick within the file.
    pub start_tick: u64,
    /// Duration in ticks.
    pub duration_ticks: u64,
    /// MIDI velocity (0–127).
    pub velocity: u8,
}

/// All notes extracted from a MIDI file, plus derived range metadata.
#[derive(Debug, Clone)]
pub struct PianoRollView {
    /// Notes sorted by start tick, then pitch.
    pub notes: Vec<PianoRollNote>,
    /// Lowest pitch present.
    pub pitch_lo: u8,
    /// Highest pitch present.
    pub pitch_hi: u8,
    /// Total length of the file in ticks.
    pub total_ticks: u64,
}

impl PianoRollView {
    /// Parses raw MIDI bytes and builds a piano-roll view.
    ///
    /// Returns [`PianoRollError::Empty`] if no note events are found.
    pub fn from_midi_bytes(bytes: &[u8]) -> Result<Self, PianoRollError> {
        let song = midi::import(bytes)?;

        let mut notes: Vec<PianoRollNote> = Vec::new();
        let mut total_ticks: u64 = 0;

        for track in &song.tracks {
            let mut cursor: u64 = 0;
            for bar in &track.phrase.bars {
                for event in &bar.events {
                    match event {
                        Event::Note(n) => {
                            let dur = u64::from(n.duration.0);
                            notes.push(PianoRollNote {
                                pitch: n.pitch.0,
                                start_tick: cursor,
                                duration_ticks: dur,
                                velocity: n.velocity.0,
                            });
                            cursor = cursor.saturating_add(dur);
                        }
                        Event::Rest(r) => {
                            cursor = cursor.saturating_add(u64::from(r.duration.0));
                        }
                    }
                }
            }
            if cursor > total_ticks {
                total_ticks = cursor;
            }
        }

        if notes.is_empty() {
            return Err(PianoRollError::Empty);
        }

        let pitch_lo = notes.iter().map(|n| n.pitch).min().unwrap_or(0);
        let pitch_hi = notes.iter().map(|n| n.pitch).max().unwrap_or(127);

        notes.sort_unstable_by_key(|n| (n.start_tick, n.pitch));

        Ok(Self {
            notes,
            pitch_lo,
            pitch_hi,
            total_ticks,
        })
    }
}
