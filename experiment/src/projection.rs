//! The canonical semantic projection, version 1.
//!
//! The one projection of the model values an experiment records (ADR-0034
//! decision 5): the experiment bundle serialises it, and every fingerprint is
//! the domain-tagged walk of it. There is no second canonicalization.
//!
//! Each `…V1` type mirrors a model type field for field, and converts both
//! ways explicitly. Model → projection is total. Projection → model validates
//! through the model's own constructors and refuses, typed, anything the model
//! cannot hold — a malformed file is never coerced into a nearby valid value.
//! Nothing in the model is left out of the projection.
//!
//! Some model fields are private (`Tempo`, `Tuning`, `NoteMarks`): the
//! projection records what their accessors observe. Changing what the
//! projection observes is a domain version bump with its golden
//! (`experiment/tests/identity_pins.rs`).

use griff_core::event::{
    ConfidenceBps, FretboardPosition, NoteMark, NoteMarks, NotePosition, Pitch, SpanTechnique,
    TechniqueEvidence, TechniqueSource, Tempo, Ticks, TimeSignature, Tuning, Velocity,
};
use griff_core::generate::{PitchMaterial, RhythmTemplate, TemplateNote};
use griff_core::generation_input::GenerationAsk;
use griff_core::gesture::GestureControl;
use griff_core::score::{
    AtomEvent, AtomNote, AtomRest, EventGroup, EventGroupKind, ImportWarning, LossReport,
    MasterBar, RepeatMarker, Score, SourceMeta, TechniqueSpan, Track, Voice,
};
use griff_core::slice::TickRange;
use griff_core::tonal::TonalContext;
use serde::{Deserialize, Serialize};

use crate::fingerprint::{Fingerprint, Hasher};

/// Microseconds in a minute: a non-integer BPM `n/d` exists only as
/// `60 000 000 / micros` reduced (`Tempo::from_micros_per_quarter`).
const MICROS_PER_MINUTE: u64 = 60_000_000;

/// Why a projection cannot become a model value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectionError {
    /// A tick range ends before it starts.
    InvalidTickRange {
        /// Recorded start.
        start: u32,
        /// Recorded end.
        end: u32,
    },
    /// A time signature the model rejects.
    InvalidTimeSignature {
        /// Recorded numerator.
        numerator: u8,
        /// Recorded denominator.
        denominator: u8,
    },
    /// A reduced BPM fraction no model constructor produces.
    UnrepresentableTempo {
        /// Recorded numerator.
        numerator: u32,
        /// Recorded denominator.
        denominator: u32,
    },
    /// A pitch above the 7-bit MIDI range.
    InvalidPitch(u8),
    /// A velocity above the 7-bit MIDI range.
    InvalidVelocity(u8),
    /// A confidence above 10 000 basis points.
    InvalidConfidence(u16),
    /// A count that does not fit this platform's `usize`.
    UnrepresentableCount(u64),
    /// A gesture rest that is not a finite number.
    NonFiniteGesture,
}

// ── score ─────────────────────────────────────────────────────────────────────

/// [`Score`], projected.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScoreV1 {
    /// Pulses per quarter note.
    pub ticks_per_quarter: u16,
    /// The master timeline.
    pub master_bars: Vec<MasterBarV1>,
    /// Instrument tracks.
    pub tracks: Vec<TrackV1>,
    /// Source metadata, when the importer recorded any.
    pub source_meta: Option<SourceMetaV1>,
    /// Import loss warnings, in order.
    pub loss: Vec<ImportWarningV1>,
}

/// A half-open tick range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TickRangeV1 {
    /// Inclusive start.
    pub start: u32,
    /// Exclusive end.
    pub end: u32,
}

/// A master bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MasterBarV1 {
    /// Bar index.
    pub index: u64,
    /// The bar's ticks.
    pub tick_range: TickRangeV1,
    /// Meter numerator.
    pub numerator: u8,
    /// Meter denominator.
    pub denominator: u8,
    /// Reduced BPM numerator (`Tempo::bpm_numerator`).
    pub bpm_numerator: u32,
    /// Reduced BPM denominator (`Tempo::bpm_denominator`).
    pub bpm_denominator: u32,
    /// Whether a repeat opens here.
    pub repeat_start: bool,
    /// The closing repeat's play count (0 when none closes here).
    pub repeat_play_count: u8,
}

/// Source metadata.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceMetaV1 {
    /// The source format label.
    pub format: Option<String>,
}

/// An import loss warning.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImportWarningV1 {
    /// A track name was not valid UTF-8.
    TrackNameInvalidUtf8 {
        /// The track.
        track_index: u64,
    },
    /// SMPTE timing is unsupported.
    SmpteTimingUnsupported,
    /// A tempo was approximated.
    TempoApproximated {
        /// The bar.
        bar_index: u64,
        /// The nearest microseconds per quarter.
        nearest_micros: u32,
    },
    /// Any other warning.
    Other {
        /// Its message.
        message: String,
    },
}

/// A track.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrackV1 {
    /// Track name.
    pub name: Option<String>,
    /// MIDI channel.
    pub channel: u8,
    /// Open-string pitches, string 1 (highest) first (`Tuning::open_strings`).
    pub tuning: Vec<u8>,
    /// Voices.
    pub voices: Vec<VoiceV1>,
}

/// A voice.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VoiceV1 {
    /// Voice id.
    pub id: u8,
    /// Event groups in order.
    pub event_groups: Vec<EventGroupV1>,
}

/// An event group.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EventGroupV1 {
    /// Group kind.
    pub kind: EventGroupKindV1,
    /// Atoms in order.
    pub atoms: Vec<AtomV1>,
    /// Technique spans in order.
    pub technique_spans: Vec<TechniqueSpanV1>,
}

/// An event group's kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EventGroupKindV1 {
    /// One atom.
    Single,
    /// A chord.
    Chord,
    /// An arpeggio.
    Arpeggio,
    /// A strum.
    Strum,
    /// A tuplet.
    Tuplet {
        /// Notes played…
        num: u8,
        /// …in the time of.
        den: u8,
    },
    /// A grace group.
    Grace,
}

/// An atom.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AtomV1 {
    /// A note.
    Note(NoteV1),
    /// A rest.
    Rest(RestV1),
}

/// A note.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NoteV1 {
    /// Onset tick.
    pub absolute_start: u32,
    /// Duration in ticks.
    pub duration: u32,
    /// MIDI pitch.
    pub pitch: u8,
    /// MIDI velocity.
    pub velocity: u8,
    /// Per-note marks.
    pub marks: MarksV1,
    /// Fretboard position, when known.
    pub position: Option<NotePositionV1>,
}

/// A note's marks, one field per `NoteMark` (in `NoteMark::ALL` order).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[allow(clippy::struct_excessive_bools)] // one bool per mark is the projection
pub struct MarksV1 {
    /// Accent.
    pub accent: bool,
    /// Ghost note.
    pub ghost: bool,
    /// Staccato.
    pub staccato: bool,
    /// Dead note.
    pub dead_note: bool,
    /// Natural harmonic.
    pub harmonic_natural: bool,
    /// Pinch harmonic.
    pub harmonic_pinch: bool,
    /// Tap.
    pub tap: bool,
}

/// A rest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RestV1 {
    /// Onset tick.
    pub absolute_start: u32,
    /// Duration in ticks.
    pub duration: u32,
}

/// A fretboard position with its evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NotePositionV1 {
    /// String, 1 = highest.
    pub string: u8,
    /// Fret.
    pub fret: u8,
    /// Why it is believed.
    pub evidence: EvidenceV1,
}

/// Technique evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceV1 {
    /// Where it came from.
    pub source: TechniqueSourceV1,
    /// Confidence in basis points.
    pub confidence_bps: u16,
}

/// Where evidence came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TechniqueSourceV1 {
    /// Stated by the source.
    Explicit,
    /// Inferred from MIDI.
    InferredFromMidi,
}

/// A technique span.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TechniqueSpanV1 {
    /// The technique.
    pub technique: SpanTechniqueV1,
    /// Its ticks.
    pub tick_range: TickRangeV1,
    /// Its evidence.
    pub evidence: EvidenceV1,
}

/// A spanning technique.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SpanTechniqueV1 {
    /// Slide.
    Slide,
    /// Bend.
    Bend,
    /// Legato.
    Legato,
    /// Palm mute.
    PalmMute,
    /// Hammer-on.
    HammerOn,
    /// Pull-off.
    PullOff,
    /// Vibrato.
    Vibrato,
    /// Let ring.
    LetRing,
}

impl From<&Score> for ScoreV1 {
    fn from(score: &Score) -> Self {
        let Score {
            ticks_per_quarter,
            master_bars,
            tracks,
            source_meta,
            loss,
        } = score;
        let LossReport { warnings } = loss;
        Self {
            ticks_per_quarter: *ticks_per_quarter,
            master_bars: master_bars.iter().map(MasterBarV1::from).collect(),
            tracks: tracks.iter().map(TrackV1::from).collect(),
            source_meta: source_meta
                .as_ref()
                .map(|SourceMeta { format }| SourceMetaV1 {
                    format: format.clone(),
                }),
            loss: warnings.iter().map(ImportWarningV1::from).collect(),
        }
    }
}

impl ScoreV1 {
    /// The model score this projection records.
    ///
    /// # Errors
    /// The first [`ProjectionError`] found.
    pub fn to_score(&self) -> Result<Score, ProjectionError> {
        let Self {
            ticks_per_quarter,
            master_bars,
            tracks,
            source_meta,
            loss,
        } = self;
        Ok(Score {
            ticks_per_quarter: *ticks_per_quarter,
            master_bars: master_bars
                .iter()
                .copied()
                .map(MasterBarV1::to_bar)
                .collect::<Result<_, _>>()?,
            tracks: tracks
                .iter()
                .map(TrackV1::to_track)
                .collect::<Result<_, _>>()?,
            source_meta: source_meta
                .as_ref()
                .map(|SourceMetaV1 { format }| SourceMeta {
                    format: format.clone(),
                }),
            loss: LossReport {
                warnings: loss.iter().map(ImportWarningV1::to_warning).collect(),
            },
        })
    }

    /// This projection's fingerprint (domain `griff.score.v1`).
    #[must_use]
    pub fn fingerprint(&self) -> Fingerprint {
        let mut h = Hasher::new("griff.score.v1");
        let Self {
            ticks_per_quarter,
            master_bars,
            tracks,
            source_meta,
            loss,
        } = self;
        h.u16(*ticks_per_quarter);
        h.usize(master_bars.len());
        for bar in master_bars {
            bar.write(&mut h);
        }
        h.usize(tracks.len());
        for track in tracks {
            track.write(&mut h);
        }
        match source_meta {
            None => h.u8(0),
            Some(SourceMetaV1 { format }) => {
                h.u8(1);
                h.option_str(format.as_deref());
            }
        }
        h.usize(loss.len());
        for warning in loss {
            warning.write(&mut h);
        }
        h.finish()
    }
}

impl From<TickRange> for TickRangeV1 {
    fn from(TickRange { start, end }: TickRange) -> Self {
        Self {
            start: start.0,
            end: end.0,
        }
    }
}

impl TickRangeV1 {
    fn to_range(self) -> Result<TickRange, ProjectionError> {
        TickRange::new(Ticks(self.start), Ticks(self.end)).map_err(|_| {
            ProjectionError::InvalidTickRange {
                start: self.start,
                end: self.end,
            }
        })
    }

    fn write(self, h: &mut Hasher) {
        let Self { start, end } = self;
        h.u32(start);
        h.u32(end);
    }
}

impl From<&MasterBar> for MasterBarV1 {
    fn from(bar: &MasterBar) -> Self {
        let MasterBar {
            index,
            tick_range,
            time_signature,
            tempo,
            repeat,
        } = *bar;
        let TimeSignature {
            numerator,
            denominator,
        } = time_signature;
        let RepeatMarker { start, play_count } = repeat;
        Self {
            index,
            tick_range: tick_range.into(),
            numerator,
            denominator,
            bpm_numerator: tempo.bpm_numerator(),
            bpm_denominator: tempo.bpm_denominator(),
            repeat_start: start,
            repeat_play_count: play_count,
        }
    }
}

impl MasterBarV1 {
    fn to_bar(self) -> Result<MasterBar, ProjectionError> {
        let Self {
            index,
            tick_range,
            numerator,
            denominator,
            bpm_numerator,
            bpm_denominator,
            repeat_start,
            repeat_play_count,
        } = self;
        Ok(MasterBar {
            index,
            tick_range: tick_range.to_range()?,
            time_signature: TimeSignature::new(numerator, denominator).map_err(|_| {
                ProjectionError::InvalidTimeSignature {
                    numerator,
                    denominator,
                }
            })?,
            tempo: tempo_from(bpm_numerator, bpm_denominator)?,
            repeat: RepeatMarker {
                start: repeat_start,
                play_count: repeat_play_count,
            },
        })
    }

    fn write(&self, h: &mut Hasher) {
        let Self {
            index,
            tick_range,
            numerator,
            denominator,
            bpm_numerator,
            bpm_denominator,
            repeat_start,
            repeat_play_count,
        } = *self;
        h.u64(index);
        tick_range.write(h);
        h.u8(numerator);
        h.u8(denominator);
        h.u32(bpm_numerator);
        h.u32(bpm_denominator);
        h.bool(repeat_start);
        h.u8(repeat_play_count);
    }
}

/// The tempo whose reduced BPM fraction is `numerator / denominator`, rebuilt
/// only through the model's own constructors: an integer BPM, or the
/// microseconds per quarter that reduce to it. Anything else is refused.
fn tempo_from(numerator: u32, denominator: u32) -> Result<Tempo, ProjectionError> {
    let refused = ProjectionError::UnrepresentableTempo {
        numerator,
        denominator,
    };
    let candidate = if denominator == 1 {
        Tempo::from_bpm_integer(numerator).ok()
    } else {
        u64::from(denominator)
            .checked_mul(MICROS_PER_MINUTE)
            .filter(|scaled| scaled.checked_rem(u64::from(numerator)) == Some(0))
            .and_then(|scaled| scaled.checked_div(u64::from(numerator)))
            .and_then(|micros| u32::try_from(micros).ok())
            .and_then(|micros| Tempo::from_micros_per_quarter(micros).ok())
    };
    candidate
        .filter(|t| t.bpm_numerator() == numerator && t.bpm_denominator() == denominator)
        .ok_or(refused)
}

impl From<&ImportWarning> for ImportWarningV1 {
    fn from(warning: &ImportWarning) -> Self {
        match warning {
            ImportWarning::TrackNameInvalidUtf8 { track_index } => Self::TrackNameInvalidUtf8 {
                track_index: *track_index,
            },
            ImportWarning::SmpteTimingUnsupported => Self::SmpteTimingUnsupported,
            ImportWarning::TempoApproximated {
                bar_index,
                nearest_micros,
            } => Self::TempoApproximated {
                bar_index: *bar_index,
                nearest_micros: *nearest_micros,
            },
            ImportWarning::Other(message) => Self::Other {
                message: message.clone(),
            },
        }
    }
}

impl ImportWarningV1 {
    fn to_warning(&self) -> ImportWarning {
        match self {
            Self::TrackNameInvalidUtf8 { track_index } => ImportWarning::TrackNameInvalidUtf8 {
                track_index: *track_index,
            },
            Self::SmpteTimingUnsupported => ImportWarning::SmpteTimingUnsupported,
            Self::TempoApproximated {
                bar_index,
                nearest_micros,
            } => ImportWarning::TempoApproximated {
                bar_index: *bar_index,
                nearest_micros: *nearest_micros,
            },
            Self::Other { message } => ImportWarning::Other(message.clone()),
        }
    }

    fn write(&self, h: &mut Hasher) {
        match self {
            Self::TrackNameInvalidUtf8 { track_index } => {
                h.u8(0);
                h.u64(*track_index);
            }
            Self::SmpteTimingUnsupported => h.u8(1),
            Self::TempoApproximated {
                bar_index,
                nearest_micros,
            } => {
                h.u8(2);
                h.u64(*bar_index);
                h.u32(*nearest_micros);
            }
            Self::Other { message } => {
                h.u8(3);
                h.str(message);
            }
        }
    }
}

fn pitch(value: u8) -> Result<Pitch, ProjectionError> {
    Pitch::new(value).map_err(|_| ProjectionError::InvalidPitch(value))
}

impl From<&Track> for TrackV1 {
    fn from(track: &Track) -> Self {
        let Track {
            name,
            channel,
            voices,
            tuning,
        } = track;
        Self {
            name: name.clone(),
            channel: *channel,
            tuning: tuning.open_strings().iter().map(|p| p.0).collect(),
            voices: voices
                .iter()
                .map(|Voice { id, event_groups }| VoiceV1 {
                    id: *id,
                    event_groups: event_groups.iter().map(EventGroupV1::from).collect(),
                })
                .collect(),
        }
    }
}

impl TrackV1 {
    fn to_track(&self) -> Result<Track, ProjectionError> {
        let Self {
            name,
            channel,
            tuning,
            voices,
        } = self;
        Ok(Track {
            name: name.clone(),
            channel: *channel,
            voices: voices
                .iter()
                .map(|VoiceV1 { id, event_groups }| {
                    Ok(Voice {
                        id: *id,
                        event_groups: event_groups
                            .iter()
                            .map(EventGroupV1::to_group)
                            .collect::<Result<_, _>>()?,
                    })
                })
                .collect::<Result<_, _>>()?,
            tuning: Tuning::new(tuning.iter().map(|&p| pitch(p)).collect::<Result<_, _>>()?),
        })
    }

    fn write(&self, h: &mut Hasher) {
        let Self {
            name,
            channel,
            tuning,
            voices,
        } = self;
        h.option_str(name.as_deref());
        h.u8(*channel);
        h.usize(tuning.len());
        for &string in tuning {
            h.u8(string);
        }
        h.usize(voices.len());
        for VoiceV1 { id, event_groups } in voices {
            h.u8(*id);
            h.usize(event_groups.len());
            for group in event_groups {
                group.write(h);
            }
        }
    }
}

impl From<&EventGroup> for EventGroupV1 {
    fn from(group: &EventGroup) -> Self {
        let EventGroup {
            kind,
            atoms,
            technique_spans,
        } = group;
        Self {
            kind: match *kind {
                EventGroupKind::Single => EventGroupKindV1::Single,
                EventGroupKind::Chord => EventGroupKindV1::Chord,
                EventGroupKind::Arpeggio => EventGroupKindV1::Arpeggio,
                EventGroupKind::Strum => EventGroupKindV1::Strum,
                EventGroupKind::Tuplet { num, den } => EventGroupKindV1::Tuplet { num, den },
                EventGroupKind::Grace => EventGroupKindV1::Grace,
            },
            atoms: atoms.iter().map(|&atom| AtomV1::from(atom)).collect(),
            technique_spans: technique_spans
                .iter()
                .map(|&span| TechniqueSpanV1::from(span))
                .collect(),
        }
    }
}

impl EventGroupV1 {
    fn to_group(&self) -> Result<EventGroup, ProjectionError> {
        let Self {
            kind,
            atoms,
            technique_spans,
        } = self;
        Ok(EventGroup {
            kind: match *kind {
                EventGroupKindV1::Single => EventGroupKind::Single,
                EventGroupKindV1::Chord => EventGroupKind::Chord,
                EventGroupKindV1::Arpeggio => EventGroupKind::Arpeggio,
                EventGroupKindV1::Strum => EventGroupKind::Strum,
                EventGroupKindV1::Tuplet { num, den } => EventGroupKind::Tuplet { num, den },
                EventGroupKindV1::Grace => EventGroupKind::Grace,
            },
            atoms: atoms
                .iter()
                .copied()
                .map(AtomV1::to_atom)
                .collect::<Result<_, _>>()?,
            technique_spans: technique_spans
                .iter()
                .copied()
                .map(TechniqueSpanV1::to_span)
                .collect::<Result<_, _>>()?,
        })
    }

    fn write(&self, h: &mut Hasher) {
        let Self {
            kind,
            atoms,
            technique_spans,
        } = self;
        match *kind {
            EventGroupKindV1::Single => h.u8(0),
            EventGroupKindV1::Chord => h.u8(1),
            EventGroupKindV1::Arpeggio => h.u8(2),
            EventGroupKindV1::Strum => h.u8(3),
            EventGroupKindV1::Tuplet { num, den } => {
                h.u8(4);
                h.u8(num);
                h.u8(den);
            }
            EventGroupKindV1::Grace => h.u8(5),
        }
        h.usize(atoms.len());
        for atom in atoms {
            atom.write(h);
        }
        h.usize(technique_spans.len());
        for span in technique_spans {
            span.write(h);
        }
    }
}

impl From<AtomEvent> for AtomV1 {
    fn from(atom: AtomEvent) -> Self {
        match atom {
            AtomEvent::Note(AtomNote {
                absolute_start,
                duration,
                pitch,
                velocity,
                marks,
                position,
            }) => Self::Note(NoteV1 {
                absolute_start: absolute_start.0,
                duration: duration.0,
                pitch: pitch.0,
                velocity: velocity.0,
                marks: MarksV1::from(marks),
                position: position.map(
                    |NotePosition {
                         position: FretboardPosition { string, fret },
                         evidence,
                     }| NotePositionV1 {
                        string,
                        fret,
                        evidence: evidence.into(),
                    },
                ),
            }),
            AtomEvent::Rest(AtomRest {
                absolute_start,
                duration,
            }) => Self::Rest(RestV1 {
                absolute_start: absolute_start.0,
                duration: duration.0,
            }),
        }
    }
}

impl AtomV1 {
    fn to_atom(self) -> Result<AtomEvent, ProjectionError> {
        Ok(match self {
            Self::Note(NoteV1 {
                absolute_start,
                duration,
                pitch: note_pitch,
                velocity,
                marks,
                position,
            }) => AtomEvent::Note(AtomNote {
                absolute_start: Ticks(absolute_start),
                duration: Ticks(duration),
                pitch: pitch(note_pitch)?,
                velocity: Velocity::new(velocity)
                    .map_err(|_| ProjectionError::InvalidVelocity(velocity))?,
                marks: marks.to_marks(),
                position: position
                    .map(
                        |NotePositionV1 {
                             string,
                             fret,
                             evidence,
                         }| {
                            Ok(NotePosition {
                                position: FretboardPosition { string, fret },
                                evidence: evidence.to_evidence()?,
                            })
                        },
                    )
                    .transpose()?,
            }),
            Self::Rest(RestV1 {
                absolute_start,
                duration,
            }) => AtomEvent::Rest(AtomRest {
                absolute_start: Ticks(absolute_start),
                duration: Ticks(duration),
            }),
        })
    }

    fn write(&self, h: &mut Hasher) {
        match *self {
            Self::Note(NoteV1 {
                absolute_start,
                duration,
                pitch,
                velocity,
                marks,
                position,
            }) => {
                h.u8(0);
                h.u32(absolute_start);
                h.u32(duration);
                h.u8(pitch);
                h.u8(velocity);
                for mark in NoteMark::ALL {
                    h.bool(marks.get(mark));
                }
                match position {
                    None => h.u8(0),
                    Some(NotePositionV1 {
                        string,
                        fret,
                        evidence,
                    }) => {
                        h.u8(1);
                        h.u8(string);
                        h.u8(fret);
                        evidence.write(h);
                    }
                }
            }
            Self::Rest(RestV1 {
                absolute_start,
                duration,
            }) => {
                h.u8(1);
                h.u32(absolute_start);
                h.u32(duration);
            }
        }
    }
}

impl From<NoteMarks> for MarksV1 {
    fn from(marks: NoteMarks) -> Self {
        let mut projected = Self::default();
        for mark in NoteMark::ALL {
            *projected.slot(mark) = marks.contains(mark);
        }
        projected
    }
}

impl MarksV1 {
    /// The field recording `mark` — exhaustive, so a new mark is a compile
    /// error here.
    const fn slot(&mut self, mark: NoteMark) -> &mut bool {
        match mark {
            NoteMark::Accent => &mut self.accent,
            NoteMark::Ghost => &mut self.ghost,
            NoteMark::Staccato => &mut self.staccato,
            NoteMark::DeadNote => &mut self.dead_note,
            NoteMark::HarmonicNatural => &mut self.harmonic_natural,
            NoteMark::HarmonicPinch => &mut self.harmonic_pinch,
            NoteMark::Tap => &mut self.tap,
        }
    }

    /// Whether `mark` is set.
    const fn get(mut self, mark: NoteMark) -> bool {
        *self.slot(mark)
    }

    fn to_marks(self) -> NoteMarks {
        NoteMark::ALL
            .into_iter()
            .filter(|&mark| self.get(mark))
            .fold(NoteMarks::empty(), NoteMarks::with)
    }
}

impl From<TechniqueEvidence> for EvidenceV1 {
    fn from(TechniqueEvidence { source, confidence }: TechniqueEvidence) -> Self {
        Self {
            source: match source {
                TechniqueSource::Explicit => TechniqueSourceV1::Explicit,
                TechniqueSource::InferredFromMidi => TechniqueSourceV1::InferredFromMidi,
            },
            confidence_bps: confidence.get(),
        }
    }
}

impl EvidenceV1 {
    fn to_evidence(self) -> Result<TechniqueEvidence, ProjectionError> {
        Ok(TechniqueEvidence {
            source: match self.source {
                TechniqueSourceV1::Explicit => TechniqueSource::Explicit,
                TechniqueSourceV1::InferredFromMidi => TechniqueSource::InferredFromMidi,
            },
            confidence: ConfidenceBps::new(self.confidence_bps)
                .map_err(|_| ProjectionError::InvalidConfidence(self.confidence_bps))?,
        })
    }

    fn write(self, h: &mut Hasher) {
        let Self {
            source,
            confidence_bps,
        } = self;
        h.u8(match source {
            TechniqueSourceV1::Explicit => 0,
            TechniqueSourceV1::InferredFromMidi => 1,
        });
        h.u16(confidence_bps);
    }
}

impl From<TechniqueSpan> for TechniqueSpanV1 {
    fn from(
        TechniqueSpan {
            technique,
            tick_range,
            evidence,
        }: TechniqueSpan,
    ) -> Self {
        Self {
            technique: match technique {
                SpanTechnique::Slide => SpanTechniqueV1::Slide,
                SpanTechnique::Bend => SpanTechniqueV1::Bend,
                SpanTechnique::Legato => SpanTechniqueV1::Legato,
                SpanTechnique::PalmMute => SpanTechniqueV1::PalmMute,
                SpanTechnique::HammerOn => SpanTechniqueV1::HammerOn,
                SpanTechnique::PullOff => SpanTechniqueV1::PullOff,
                SpanTechnique::Vibrato => SpanTechniqueV1::Vibrato,
                SpanTechnique::LetRing => SpanTechniqueV1::LetRing,
            },
            tick_range: tick_range.into(),
            evidence: evidence.into(),
        }
    }
}

impl TechniqueSpanV1 {
    fn to_span(self) -> Result<TechniqueSpan, ProjectionError> {
        Ok(TechniqueSpan {
            technique: match self.technique {
                SpanTechniqueV1::Slide => SpanTechnique::Slide,
                SpanTechniqueV1::Bend => SpanTechnique::Bend,
                SpanTechniqueV1::Legato => SpanTechnique::Legato,
                SpanTechniqueV1::PalmMute => SpanTechnique::PalmMute,
                SpanTechniqueV1::HammerOn => SpanTechnique::HammerOn,
                SpanTechniqueV1::PullOff => SpanTechnique::PullOff,
                SpanTechniqueV1::Vibrato => SpanTechnique::Vibrato,
                SpanTechniqueV1::LetRing => SpanTechnique::LetRing,
            },
            tick_range: self.tick_range.to_range()?,
            evidence: self.evidence.to_evidence()?,
        })
    }

    fn write(&self, h: &mut Hasher) {
        let Self {
            technique,
            tick_range,
            evidence,
        } = *self;
        h.u8(match technique {
            SpanTechniqueV1::Slide => 0,
            SpanTechniqueV1::Bend => 1,
            SpanTechniqueV1::Legato => 2,
            SpanTechniqueV1::PalmMute => 3,
            SpanTechniqueV1::HammerOn => 4,
            SpanTechniqueV1::PullOff => 5,
            SpanTechniqueV1::Vibrato => 6,
            SpanTechniqueV1::LetRing => 7,
        });
        tick_range.write(h);
        evidence.write(h);
    }
}

// ── generation inputs ────────────────────────────────────────────────────────

/// A rhythm template.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RhythmTemplateV1 {
    /// Placed notes as `(offset, duration)` ticks, in template order.
    pub notes: Vec<TemplateNoteV1>,
}

/// A placed template note.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TemplateNoteV1 {
    /// Offset from the bar start.
    pub offset: u32,
    /// Duration.
    pub duration: u32,
}

/// A gesture ask.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GestureControlV1 {
    /// Notes per burst.
    pub burst_notes: u64,
    /// Rest length in quarters.
    pub rest_quarters: f64,
}

/// A scale.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PitchMaterialV1 {
    /// Root pitch.
    pub root: u8,
    /// Semitone intervals from the root, in order.
    pub intervals: Vec<u8>,
}

/// A generation ask.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GenerationAskV1 {
    /// Deterministic seed.
    pub seed: u64,
    /// Bars.
    pub bars: u64,
    /// Seed variants per strategy.
    pub variants_per_strategy: u64,
    /// Whether gesture carving was requested.
    pub gesture: bool,
    /// The carried tonal context, in its own serde form.
    pub tonal: Option<TonalContext>,
}

impl From<&RhythmTemplate> for RhythmTemplateV1 {
    fn from(RhythmTemplate { notes }: &RhythmTemplate) -> Self {
        Self {
            notes: notes
                .iter()
                .map(|&TemplateNote { offset, duration }| TemplateNoteV1 {
                    offset: offset.0,
                    duration: duration.0,
                })
                .collect(),
        }
    }
}

impl From<GestureControl> for GestureControlV1 {
    fn from(
        GestureControl {
            burst_notes,
            rest_quarters,
        }: GestureControl,
    ) -> Self {
        Self {
            burst_notes: u64::try_from(burst_notes).unwrap_or(u64::MAX),
            rest_quarters,
        }
    }
}

impl From<&PitchMaterial> for PitchMaterialV1 {
    fn from(PitchMaterial { root, intervals }: &PitchMaterial) -> Self {
        Self {
            root: root.0,
            intervals: intervals.clone(),
        }
    }
}

impl From<&GenerationAsk> for GenerationAskV1 {
    fn from(
        &GenerationAsk {
            seed,
            bars,
            variants_per_strategy,
            gesture,
            tonal,
        }: &GenerationAsk,
    ) -> Self {
        Self {
            seed,
            bars: u64::try_from(bars).unwrap_or(u64::MAX),
            variants_per_strategy: u64::try_from(variants_per_strategy).unwrap_or(u64::MAX),
            gesture,
            tonal,
        }
    }
}

impl RhythmTemplateV1 {
    /// The fingerprint of an ordered palette (domain `griff.rhythms.v1`).
    #[must_use]
    pub fn fingerprint_all(palette: &[Self]) -> Fingerprint {
        let mut h = Hasher::new("griff.rhythms.v1");
        h.usize(palette.len());
        for Self { notes } in palette {
            h.usize(notes.len());
            for &TemplateNoteV1 { offset, duration } in notes {
                h.u32(offset);
                h.u32(duration);
            }
        }
        h.finish()
    }

    /// The model template.
    #[must_use]
    pub fn to_template(&self) -> RhythmTemplate {
        RhythmTemplate {
            notes: self
                .notes
                .iter()
                .map(|&TemplateNoteV1 { offset, duration }| TemplateNote {
                    offset: Ticks(offset),
                    duration: Ticks(duration),
                })
                .collect(),
        }
    }
}

impl GestureControlV1 {
    /// The fingerprint of a gesture channel, `None` included (domain
    /// `griff.gesture.v1`).
    #[must_use]
    pub fn fingerprint_option(gesture: Option<Self>) -> Fingerprint {
        let mut h = Hasher::new("griff.gesture.v1");
        match gesture {
            None => h.u8(0),
            Some(Self {
                burst_notes,
                rest_quarters,
            }) => {
                h.u8(1);
                h.u64(burst_notes);
                h.f64(rest_quarters);
            }
        }
        h.finish()
    }

    /// The model gesture.
    ///
    /// # Errors
    /// [`ProjectionError::UnrepresentableCount`] or
    /// [`ProjectionError::NonFiniteGesture`].
    pub fn to_gesture(&self) -> Result<GestureControl, ProjectionError> {
        if !self.rest_quarters.is_finite() {
            return Err(ProjectionError::NonFiniteGesture);
        }
        Ok(GestureControl {
            burst_notes: count(self.burst_notes)?,
            rest_quarters: self.rest_quarters,
        })
    }
}

impl PitchMaterialV1 {
    /// Writes this scale into a fingerprint.
    pub(crate) fn write(&self, h: &mut Hasher) {
        let Self { root, intervals } = self;
        h.u8(*root);
        h.usize(intervals.len());
        for &interval in intervals {
            h.u8(interval);
        }
    }

    /// The model scale.
    ///
    /// # Errors
    /// [`ProjectionError::InvalidPitch`] for a root above 127.
    pub fn to_material(&self) -> Result<PitchMaterial, ProjectionError> {
        Ok(PitchMaterial {
            root: pitch(self.root)?,
            intervals: self.intervals.clone(),
        })
    }
}

impl GenerationAskV1 {
    /// This ask's fingerprint (domain `griff.ask.v1`).
    #[must_use]
    pub fn fingerprint(&self) -> Fingerprint {
        let mut h = Hasher::new("griff.ask.v1");
        let Self {
            seed,
            bars,
            variants_per_strategy,
            gesture,
            tonal,
        } = *self;
        h.u64(seed);
        h.u64(bars);
        h.u64(variants_per_strategy);
        h.bool(gesture);
        // The tonal context's own serde projection: the one form
        // `griff_core::tonal` itself keeps stable.
        let tonal = tonal.and_then(|t| serde_json::to_string(&t).ok());
        h.option_str(tonal.as_deref());
        h.finish()
    }

    /// The model ask.
    ///
    /// # Errors
    /// [`ProjectionError::UnrepresentableCount`].
    pub fn to_ask(&self) -> Result<GenerationAsk, ProjectionError> {
        Ok(GenerationAsk {
            seed: self.seed,
            bars: count(self.bars)?,
            variants_per_strategy: count(self.variants_per_strategy)?,
            gesture: self.gesture,
            tonal: self.tonal,
        })
    }
}

/// A recorded count on this platform.
pub(crate) fn count(value: u64) -> Result<usize, ProjectionError> {
    usize::try_from(value).map_err(|_| ProjectionError::UnrepresentableCount(value))
}
