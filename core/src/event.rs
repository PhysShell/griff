//! Fundamental musical event types.

/// MIDI pitch number (0–127; 60 = middle C).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Pitch(pub u8);

/// Duration in ticks (PPQN-relative; track resolution is carried externally).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Ticks(pub u32);

/// MIDI velocity (0–127).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Velocity(pub u8);

/// Tempo in beats per minute.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
pub struct Tempo(pub f64);

/// Time signature, e.g. 4/4 or 7/8.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TimeSignature {
    /// Beats per measure.
    pub numerator: u8,
    /// Beat unit as a power of two (2 = half-note, 4 = quarter-note, …).
    pub denominator: u8,
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
    pub fn duration(self) -> Ticks {
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

/// An ordered sequence of bars forming a musical phrase.
#[derive(Debug, Clone, PartialEq)]
pub struct Phrase {
    /// Bars in order.
    pub bars: Vec<Bar>,
}

#[cfg(test)]
mod tests {
    use super::{
        Articulation, Bar, Event, Note, Phrase, Pitch, Rest, Tempo, Ticks, TimeSignature,
        Velocity,
    };

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
        let rest = Rest { duration: Ticks(240) };
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
            time_signature: TimeSignature { numerator: 4, denominator: 4 },
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
    fn phrase_collects_bars() {
        let bar = Bar {
            time_signature: TimeSignature { numerator: 7, denominator: 8 },
            tempo: Tempo(140.0),
            events: vec![Event::Rest(Rest { duration: Ticks(1920) })],
        };
        let phrase = Phrase { bars: vec![bar] };
        assert_eq!(
            phrase.bars.len(),
            1,
            "phrase must contain the one bar that was added",
        );
    }
}
