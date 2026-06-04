//! Canonical score model — the internal musical representation for griff.
//!
//! Introduced in S1 (ADR-0002); the single internal truth (ADR-0011).
//!
//! ## Hierarchy
//!
//! | Concept             | Role                                      |
//! |---------------------|-------------------------------------------|
//! | `Score`             | top-level document                        |
//! | `MasterBar`         | shared transport (meter / tempo)          |
//! | `Track`             | an instrument part                        |
//! | `Voice`             | a polyphonic event stream                 |
//! | `EventGroup`        | chord / tuplet / grace                    |
//! | `AtomEvent::Note`   | `AtomNote`                                 |
//! | `AtomEvent::Rest`   | `AtomRest`                                 |
//! | `TechniqueSpan`     | span-level technique                      |
//! | `SourceMeta`        | importer evidence                         |
//! | `LossReport`        | import/export losses                      |

use crate::{
    event::{
        Articulation, FretboardPosition, NoteMarks, Pitch, Tempo, Ticks, TimeSignature, Tuning,
        Velocity,
    },
    slice::TickRange,
};

// ── loss reporting ─────────────────────────────────────────────────────────────

/// A loss or approximation incurred during format import or export.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ImportWarning {
    /// A track name was present but could not be decoded as UTF-8.
    TrackNameInvalidUtf8 {
        /// Zero-based index of the affected track.
        track_index: usize,
    },
    /// The MIDI file used SMPTE/timecode timing; `griff` does not yet support
    /// it.
    SmpteTimingUnsupported,
    /// Any other loss not covered by the variants above.
    Other(String),
}

/// Ordered list of losses gathered during a format import or export.
///
/// An empty report means a clean, lossless conversion.
#[derive(Debug, Clone, Default)]
pub struct LossReport {
    /// Ordered warnings; empty when conversion was lossless.
    pub warnings: Vec<ImportWarning>,
}

impl LossReport {
    /// Creates an empty loss report.
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends a warning.
    pub fn add(&mut self, warning: ImportWarning) {
        self.warnings.push(warning);
    }

    /// Returns `true` when no losses were recorded.
    pub fn is_clean(&self) -> bool {
        self.warnings.is_empty()
    }

    /// Drains `other`'s warnings into `self`.
    pub fn absorb(&mut self, mut other: Self) {
        self.warnings.append(&mut other.warnings);
    }
}

// ── source metadata ────────────────────────────────────────────────────────────

/// Format-level metadata carried alongside the canonical model.
#[derive(Debug, Clone, Default)]
pub struct SourceMeta {
    /// Identifier of the originating format, e.g. `"MIDI"`, `"GP5"`.
    pub format: Option<String>,
}

// ── technique spans ────────────────────────────────────────────────────────────

/// A playing technique applied over a tick range within a voice.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TechniqueSpan {
    /// The technique in effect.
    pub technique: Articulation,
    /// Absolute tick range over which the technique applies.
    pub tick_range: TickRange,
}

// ── atom events ───────────────────────────────────────────────────────────────

/// A note event in the canonical model, carrying absolute position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AtomNote {
    /// Absolute tick onset.
    pub absolute_start: Ticks,
    /// Duration in ticks.
    pub duration: Ticks,
    /// MIDI pitch (0–127).
    pub pitch: Pitch,
    /// MIDI velocity (0–127).
    pub velocity: Velocity,
    /// Per-note technique marks — a `Copy` set of co-occurring [`NoteMarks`]
    /// (ADR-0018), replacing the former single `Option<Articulation>`. Spanning
    /// techniques live on [`TechniqueSpan`].
    pub marks: NoteMarks,
    /// Optional fretboard position under the track's [`Tuning`] (ADR-0018).
    /// Guitar Pro supplies it; MIDI import leaves it `None`.
    pub position: Option<FretboardPosition>,
}

/// A rest (silence) in the canonical model, carrying absolute position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AtomRest {
    /// Absolute tick onset.
    pub absolute_start: Ticks,
    /// Duration in ticks.
    pub duration: Ticks,
}

/// The minimal event unit in the canonical model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AtomEvent {
    /// A sounding note.
    Note(AtomNote),
    /// A silence.
    Rest(AtomRest),
}

impl AtomEvent {
    /// Absolute tick onset of this event.
    pub const fn absolute_start(self) -> Ticks {
        match self {
            Self::Note(n) => n.absolute_start,
            Self::Rest(r) => r.absolute_start,
        }
    }

    /// Duration of this event in ticks.
    pub const fn duration(self) -> Ticks {
        match self {
            Self::Note(n) => n.duration,
            Self::Rest(r) => r.duration,
        }
    }
}

// ── event group ────────────────────────────────────────────────────────────────

/// How the atoms in an [`EventGroup`] relate to each other musically.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EventGroupKind {
    /// A single, isolated event.
    Single,
    /// Simultaneous notes forming a chord.
    Chord,
    /// A broken chord — notes sounded sequentially.
    Arpeggio,
    /// A strummed chord with per-string micro-timing offsets.
    Strum,
    /// A rhythmic tuplet fitting an off-grid subdivision.
    Tuplet {
        /// Note count within the tuplet (e.g. 3 for a triplet).
        num: u8,
        /// Grid subdivision the tuplet fits into.
        den: u8,
    },
    /// Grace notes preceding the main event.
    Grace,
}

/// A group of [`AtomEvent`]s sharing a musical role, plus optional technique spans.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventGroup {
    /// Structural role of this group.
    pub kind: EventGroupKind,
    /// Atomic events that make up the group.
    pub atoms: Vec<AtomEvent>,
    /// Technique spans scoped to this group's tick range.
    pub technique_spans: Vec<TechniqueSpan>,
}

// ── voice, track, master bar, score ───────────────────────────────────────────

/// An independent event stream within a track (needed for polyphony).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Voice {
    /// Voice identifier; 0 is the primary voice.
    pub id: u8,
    /// Ordered event groups.
    pub event_groups: Vec<EventGroup>,
}

/// An instrument part or logical track layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Track {
    /// Optional track name from the source format.
    pub name: Option<String>,
    /// Dominant MIDI channel (0–15).
    pub channel: u8,
    /// Independent event streams; note-bearing tracks have at least one.
    pub voices: Vec<Voice>,
    /// Tuning used to interpret note fretboard positions (ADR-0018); defaults to
    /// Standard E (ADR-0006) when the source carries no tuning.
    pub tuning: Tuning,
}

/// A score-level bar whose meter and tempo are shared across all tracks.
///
/// `MasterBar` is the single source of truth for transport (ADR-0003).
#[derive(Debug, Clone, Copy)]
pub struct MasterBar {
    /// Zero-based bar index.
    pub index: usize,
    /// Absolute half-open tick range `[start, end)` of this bar.
    pub tick_range: TickRange,
    /// Meter of this bar.
    pub time_signature: TimeSignature,
    /// Tempo at the start of this bar in BPM.
    pub tempo: Tempo,
}

/// Top-level musical document in the canonical model.
///
/// `Score` is the root of the canonical hierarchy:
/// `Score → MasterBar` (transport) and `Score → Track → Voice → EventGroup → AtomEvent` (content).
#[derive(Debug, Clone)]
pub struct Score {
    /// Tick resolution — pulses per quarter note.
    pub ticks_per_quarter: u16,
    /// Score-level bars; own all tempo and meter information (ADR-0003).
    pub master_bars: Vec<MasterBar>,
    /// Instrument tracks.
    pub tracks: Vec<Track>,
    /// Optional format-level source metadata.
    pub source_meta: Option<SourceMeta>,
    /// Losses incurred during import.
    pub loss: LossReport,
}

// ── compatibility projection ─────────────────────────────────────────────────

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::missing_assert_message
)]
mod tests {
    use super::{AtomEvent, AtomNote, AtomRest, ImportWarning, LossReport, TechniqueSpan};
    use crate::{
        event::{Articulation, NoteMarks, Pitch, Ticks, Velocity},
        slice::TickRange,
    };

    fn tick_range(start: u32, end: u32) -> TickRange {
        TickRange::new(Ticks(start), Ticks(end)).expect("ordered range")
    }

    fn atom_note(start: u32, dur: u32, pitch: u8) -> AtomEvent {
        AtomEvent::Note(AtomNote {
            absolute_start: Ticks(start),
            duration: Ticks(dur),
            pitch: Pitch::new(pitch).expect("valid pitch"),
            velocity: Velocity::new(80).expect("valid velocity"),
            marks: NoteMarks::empty(),
            position: None,
        })
    }

    fn atom_rest(start: u32, dur: u32) -> AtomEvent {
        AtomEvent::Rest(AtomRest {
            absolute_start: Ticks(start),
            duration: Ticks(dur),
        })
    }

    // ── LossReport ────────────────────────────────────────────────────────────

    #[test]
    fn loss_report_starts_clean() {
        let report = LossReport::new();
        assert!(report.is_clean(), "a fresh LossReport must be clean");
        assert_eq!(report.warnings.len(), 0, "no warnings initially");
    }

    #[test]
    fn loss_report_add_and_absorb() {
        let mut a = LossReport::new();
        a.add(ImportWarning::SmpteTimingUnsupported);
        assert!(!a.is_clean());

        let mut b = LossReport::new();
        b.add(ImportWarning::Other("x".to_owned()));
        a.absorb(b);
        assert_eq!(
            a.warnings.len(),
            2,
            "absorb must drain the other report's warnings"
        );
    }

    // ── AtomEvent accessors ───────────────────────────────────────────────────

    #[test]
    fn atom_event_accessors_match_inner_fields() {
        let note = atom_note(100, 240, 60);
        assert_eq!(
            note.absolute_start(),
            Ticks(100),
            "note absolute_start accessor"
        );
        assert_eq!(note.duration(), Ticks(240), "note duration accessor");

        let rest = atom_rest(340, 120);
        assert_eq!(
            rest.absolute_start(),
            Ticks(340),
            "rest absolute_start accessor"
        );
        assert_eq!(rest.duration(), Ticks(120), "rest duration accessor");
    }

    // ── TechniqueSpan ─────────────────────────────────────────────────────────

    #[test]
    fn technique_span_stores_fields() {
        let span = TechniqueSpan {
            technique: Articulation::PalmMute,
            tick_range: tick_range(0, 480),
        };
        assert_eq!(span.technique, Articulation::PalmMute);
    }
}
