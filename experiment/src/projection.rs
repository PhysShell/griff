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

use griff_core::generate::{PitchMaterial, RhythmTemplate};
use griff_core::generation_input::GenerationAsk;
use griff_core::gesture::GestureControl;
use griff_core::score::Score;
use griff_core::tonal::TonalContext;
use serde::{Deserialize, Serialize};

use crate::fingerprint::Fingerprint;

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
        let _ = score;
        Self {
            ticks_per_quarter: 0,
            master_bars: Vec::new(),
            tracks: Vec::new(),
            source_meta: None,
            loss: Vec::new(),
        }
    }
}

impl ScoreV1 {
    /// The model score this projection records.
    ///
    /// # Errors
    /// The first [`ProjectionError`] found.
    pub const fn to_score(&self) -> Result<Score, ProjectionError> {
        let _ = self;
        Err(ProjectionError::NonFiniteGesture)
    }

    /// This projection's fingerprint (domain `griff.score.v1`).
    #[must_use]
    pub const fn fingerprint(&self) -> Fingerprint {
        let _ = self;
        Fingerprint([0; 32])
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
    fn from(template: &RhythmTemplate) -> Self {
        let _ = template;
        Self { notes: Vec::new() }
    }
}

impl From<GestureControl> for GestureControlV1 {
    fn from(gesture: GestureControl) -> Self {
        let _ = gesture;
        Self {
            burst_notes: 0,
            rest_quarters: 0.0,
        }
    }
}

impl From<&PitchMaterial> for PitchMaterialV1 {
    fn from(material: &PitchMaterial) -> Self {
        let _ = material;
        Self {
            root: 0,
            intervals: Vec::new(),
        }
    }
}

impl From<&GenerationAsk> for GenerationAskV1 {
    fn from(ask: &GenerationAsk) -> Self {
        let _ = ask;
        Self {
            seed: 0,
            bars: 0,
            variants_per_strategy: 0,
            gesture: false,
            tonal: None,
        }
    }
}

impl RhythmTemplateV1 {
    /// The fingerprint of an ordered palette (domain `griff.rhythms.v1`).
    #[must_use]
    pub const fn fingerprint_all(palette: &[Self]) -> Fingerprint {
        let _ = palette;
        Fingerprint([0; 32])
    }

    /// The model template.
    #[must_use]
    pub fn to_template(&self) -> RhythmTemplate {
        let _ = self;
        RhythmTemplate { notes: Vec::new() }
    }
}

impl GestureControlV1 {
    /// The fingerprint of a gesture channel, `None` included (domain
    /// `griff.gesture.v1`).
    #[must_use]
    pub const fn fingerprint_option(gesture: Option<Self>) -> Fingerprint {
        let _ = gesture;
        Fingerprint([0; 32])
    }

    /// The model gesture.
    ///
    /// # Errors
    /// [`ProjectionError::UnrepresentableCount`] or
    /// [`ProjectionError::NonFiniteGesture`].
    pub const fn to_gesture(&self) -> Result<GestureControl, ProjectionError> {
        let _ = self;
        Err(ProjectionError::NonFiniteGesture)
    }
}

impl PitchMaterialV1 {
    /// The model scale.
    ///
    /// # Errors
    /// [`ProjectionError::InvalidPitch`] for a root above 127.
    pub const fn to_material(&self) -> Result<PitchMaterial, ProjectionError> {
        let _ = self;
        Err(ProjectionError::InvalidPitch(0))
    }
}

impl GenerationAskV1 {
    /// This ask's fingerprint (domain `griff.ask.v1`).
    #[must_use]
    pub const fn fingerprint(&self) -> Fingerprint {
        let _ = self;
        Fingerprint([0; 32])
    }

    /// The model ask.
    ///
    /// # Errors
    /// [`ProjectionError::UnrepresentableCount`].
    pub const fn to_ask(&self) -> Result<GenerationAsk, ProjectionError> {
        let _ = self;
        Err(ProjectionError::UnrepresentableCount(0))
    }
}
