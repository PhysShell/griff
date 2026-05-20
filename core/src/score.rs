//! Canonical score model — the internal musical representation for griff.
//!
//! Introduced in S1 (ADR-0002). The legacy `Phrase/Bar/Event` model (in
//! [`crate::event`]) is retained as a compatibility layer and can be obtained
//! via [`project_phrase`].
//!
//! ## Old ↔ new mapping
//!
//! | Old (legacy)        | New (canonical)                           |
//! |---------------------|-------------------------------------------|
//! | `Phrase`            | first-voice projection of a `Track`      |
//! | `Bar`               | driven by `MasterBar` in the projection  |
//! | `Event::Note`       | `AtomEvent::Note(AtomNote)`               |
//! | `Event::Rest`       | `AtomEvent::Rest(AtomRest)`               |
//! | —                   | `Voice` — polyphonic event stream        |
//! | —                   | `EventGroup` — chord / tuplet / grace    |
//! | —                   | `TechniqueSpan` — span-level technique   |
//! | —                   | `SourceMeta` — importer evidence         |
//! | —                   | `LossReport` — import/export losses      |

use crate::{
    event::Pitch,
    event::{Articulation, Bar, Event, Note, Phrase, Rest, Tempo, Ticks, TimeSignature, Velocity},
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
    /// Optional playing technique.
    pub articulation: Option<Articulation>,
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

/// Projects the first voice of `track_index` in `score` onto the legacy
/// [`Phrase`] / [`Bar`] / [`Event`] model.
///
/// Returns `None` if `track_index` is out of bounds or the track has no voices.
///
/// The projection is intentionally lossy:
/// - Only the first voice is represented; all other voices are discarded.
/// - [`EventGroup`] structure (chord, tuplet, …) is flattened.
/// - [`TechniqueSpan`]s are not represented in the legacy model.
pub fn project_phrase(score: &Score, track_index: usize) -> Option<Phrase> {
    let track = score.tracks.get(track_index)?;
    let voice = track.voices.first()?;

    // Collect all AtomEvents from the first voice, sorted by absolute start.
    let mut atoms: Vec<AtomEvent> = voice
        .event_groups
        .iter()
        .flat_map(|g| g.atoms.iter().copied())
        .collect();
    atoms.sort_unstable_by_key(|a| a.absolute_start().0);

    let bars: Vec<Bar> = score
        .master_bars
        .iter()
        .map(|mb| {
            let in_bar: Vec<AtomEvent> = atoms
                .iter()
                .filter(|a| {
                    a.absolute_start().0 >= mb.tick_range.start.0
                        && a.absolute_start().0 < mb.tick_range.end.0
                })
                .copied()
                .collect();

            let events = fill_bar_events(&in_bar, mb.tick_range.start, mb.tick_range.end);
            Bar {
                time_signature: mb.time_signature,
                tempo: mb.tempo,
                events,
            }
        })
        .collect();

    Some(Phrase { bars })
}

/// Fills the half-open range `[bar_start, bar_end)` with legacy [`Event`]s,
/// inserting [`Event::Rest`] for any gaps between atoms.
fn fill_bar_events(atoms: &[AtomEvent], bar_start: Ticks, bar_end: Ticks) -> Vec<Event> {
    let mut events: Vec<Event> = Vec::new();
    let mut cursor = bar_start;

    for atom in atoms {
        let start = atom.absolute_start();
        if start.0 > cursor.0 {
            events.push(Event::Rest(Rest {
                duration: Ticks(start.0.saturating_sub(cursor.0)),
            }));
        }
        match *atom {
            AtomEvent::Note(n) => {
                events.push(Event::Note(Note {
                    pitch: n.pitch,
                    duration: n.duration,
                    velocity: n.velocity,
                    articulation: n.articulation,
                }));
                cursor = Ticks(start.0.saturating_add(n.duration.0));
            }
            AtomEvent::Rest(r) => {
                events.push(Event::Rest(Rest {
                    duration: r.duration,
                }));
                cursor = Ticks(start.0.saturating_add(r.duration.0));
            }
        }
    }

    if cursor.0 < bar_end.0 {
        events.push(Event::Rest(Rest {
            duration: Ticks(bar_end.0.saturating_sub(cursor.0)),
        }));
    }

    events
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
#[allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::indexing_slicing,
    clippy::missing_assert_message
)]
mod tests {
    use super::{
        project_phrase, AtomEvent, AtomNote, AtomRest, EventGroup, EventGroupKind, ImportWarning,
        LossReport, MasterBar, Score, SourceMeta, TechniqueSpan, Track, Voice,
    };
    use crate::{
        event::{Articulation, Event, Pitch, Tempo, Ticks, TimeSignature, Velocity},
        slice::TickRange,
    };

    fn ts_4_4() -> TimeSignature {
        TimeSignature::new(4, 4).expect("4/4 valid")
    }

    fn tempo_120() -> Tempo {
        Tempo::new(120.0).expect("120 BPM valid")
    }

    fn tick_range(start: u32, end: u32) -> TickRange {
        TickRange::new(Ticks(start), Ticks(end)).expect("ordered range")
    }

    fn atom_note(start: u32, dur: u32, pitch: u8) -> AtomEvent {
        AtomEvent::Note(AtomNote {
            absolute_start: Ticks(start),
            duration: Ticks(dur),
            pitch: Pitch::new(pitch).expect("valid pitch"),
            velocity: Velocity::new(80).expect("valid velocity"),
            articulation: None,
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

    // ── project_phrase ────────────────────────────────────────────────────────

    fn one_bar_score(atoms: Vec<AtomEvent>) -> Score {
        let group = EventGroup {
            kind: EventGroupKind::Single,
            atoms,
            technique_spans: Vec::new(),
        };
        let voice = Voice {
            id: 0,
            event_groups: vec![group],
        };
        let track = Track {
            name: Some("Test".to_owned()),
            channel: 0,
            voices: vec![voice],
        };
        let mb = MasterBar {
            index: 0,
            tick_range: tick_range(0, 1920),
            time_signature: ts_4_4(),
            tempo: tempo_120(),
        };
        Score {
            ticks_per_quarter: 480,
            master_bars: vec![mb],
            tracks: vec![track],
            source_meta: Some(SourceMeta {
                format: Some("test".to_owned()),
            }),
            loss: LossReport::new(),
        }
    }

    #[test]
    fn project_phrase_returns_none_for_out_of_bounds_track() {
        let score = one_bar_score(Vec::new());
        assert!(
            project_phrase(&score, 1).is_none(),
            "out-of-bounds track index must return None"
        );
    }

    #[test]
    fn project_phrase_empty_voice_fills_bar_with_rest() {
        let score = one_bar_score(Vec::new());
        let phrase = project_phrase(&score, 0).expect("track 0 exists");

        assert_eq!(phrase.bars.len(), 1, "one master bar → one projected bar");
        let bar = &phrase.bars[0];
        assert_eq!(
            bar.events.len(),
            1,
            "empty voice bar must contain a single fill-rest"
        );
        assert!(
            matches!(bar.events[0], Event::Rest(_)),
            "fill event must be a rest"
        );
    }

    #[test]
    fn project_phrase_single_note_at_bar_start() {
        let score = one_bar_score(vec![atom_note(0, 480, 60)]);
        let phrase = project_phrase(&score, 0).expect("track 0 exists");

        let bar = &phrase.bars[0];
        assert!(
            matches!(bar.events[0], Event::Note(_)),
            "first event must be the note"
        );
        assert_eq!(
            bar.events.len(),
            2,
            "note + tail rest = 2 events in projected bar"
        );
    }

    #[test]
    fn project_phrase_note_with_leading_gap_inserts_rest() {
        // Note starts at tick 240 inside a 1920-tick bar.
        let score = one_bar_score(vec![atom_note(240, 480, 62)]);
        let phrase = project_phrase(&score, 0).expect("track 0 exists");

        let bar = &phrase.bars[0];
        assert!(
            matches!(bar.events[0], Event::Rest(_)),
            "leading gap must produce a leading rest"
        );
        assert!(
            matches!(bar.events[1], Event::Note(_)),
            "second event must be the note"
        );
    }

    #[test]
    fn project_phrase_atoms_sorted_by_absolute_start() {
        // Provide atoms out of order; projection must sort them.
        let atoms = vec![atom_note(480, 480, 64), atom_note(0, 480, 60)];
        let score = one_bar_score(atoms);
        let phrase = project_phrase(&score, 0).expect("track 0 exists");

        let notes: Vec<u8> = phrase.bars[0]
            .events
            .iter()
            .filter_map(|e| match e {
                Event::Note(n) => Some(n.pitch.0),
                Event::Rest(_) => None,
            })
            .collect();

        assert_eq!(
            notes,
            vec![60, 64],
            "notes must appear in onset order regardless of insertion order"
        );
    }

    #[test]
    fn project_phrase_multi_bar_score() {
        let group = EventGroup {
            kind: EventGroupKind::Single,
            atoms: vec![atom_note(0, 480, 60), atom_note(1920, 480, 62)],
            technique_spans: Vec::new(),
        };
        let voice = Voice {
            id: 0,
            event_groups: vec![group],
        };
        let track = Track {
            name: None,
            channel: 0,
            voices: vec![voice],
        };
        let score = Score {
            ticks_per_quarter: 480,
            master_bars: vec![
                MasterBar {
                    index: 0,
                    tick_range: tick_range(0, 1920),
                    time_signature: ts_4_4(),
                    tempo: tempo_120(),
                },
                MasterBar {
                    index: 1,
                    tick_range: tick_range(1920, 3840),
                    time_signature: ts_4_4(),
                    tempo: tempo_120(),
                },
            ],
            tracks: vec![track],
            source_meta: None,
            loss: LossReport::new(),
        };

        let phrase = project_phrase(&score, 0).expect("track 0 exists");
        assert_eq!(phrase.bars.len(), 2, "two master bars → two projected bars");

        let bar0_notes: usize = phrase.bars[0]
            .events
            .iter()
            .filter(|e| matches!(e, Event::Note(_)))
            .count();
        let bar1_notes: usize = phrase.bars[1]
            .events
            .iter()
            .filter(|e| matches!(e, Event::Note(_)))
            .count();
        assert_eq!(bar0_notes, 1, "bar 0 must contain exactly one note");
        assert_eq!(bar1_notes, 1, "bar 1 must contain exactly one note");
    }

    #[test]
    fn project_phrase_atom_rest_projects_correctly() {
        let score = one_bar_score(vec![atom_rest(0, 240), atom_note(240, 480, 60)]);
        let phrase = project_phrase(&score, 0).expect("track 0 exists");

        let bar = &phrase.bars[0];
        assert!(
            matches!(bar.events[0], Event::Rest(_)),
            "first projected event must be the rest atom"
        );
        assert!(
            matches!(bar.events[1], Event::Note(_)),
            "second projected event must be the note"
        );
    }
}
