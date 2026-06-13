//! Normalized score dump — a canonical, comparison-friendly projection of a
//! [`Score`] for validation tooling (ADR-0020).
//!
//! This is a *second projection* of the canonical model, deliberately lossy:
//! it carries only the fields a Guitar-Pro-import validator compares, in a
//! shape that mirrors Guitar Pro's `measure → voice → beat → note` nesting so a
//! reference parser (`PyGuitarPro`) and griff's importer can be diffed at the
//! same semantic level. It is **not** the canonical model and never round-trips
//! back into one.
//!
//! Canonicalisation (SPEC §6): notes are bucketed into master bars by onset and
//! sorted by `(onset, pitch, string, fret)`, so group structure and import
//! ordering do not affect the output. Empty voices are dropped; transport-only
//! bars stay (their meter/tempo is meaningful).

use serde::Serialize;

use crate::score::Score;

/// A normalized, serializable projection of a whole [`Score`] (ADR-0020).
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NormalizedScore {
    /// Tick resolution — pulses per quarter note.
    pub ppqn: u16,
    /// Import losses as stable labels (empty = clean import).
    pub loss: Vec<String>,
    /// Instrument tracks.
    pub tracks: Vec<NormTrack>,
}

/// A normalized track: its tuning and per-bar content.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NormTrack {
    /// Optional track name from the source.
    pub name: Option<String>,
    /// Dominant MIDI channel (0–15).
    pub channel: u8,
    /// Open-string MIDI pitches, string 1 (highest) first.
    pub tuning: Vec<u8>,
    /// Bars in transport order; share the score's master timeline.
    pub bars: Vec<NormBar>,
}

/// A normalized bar: shared transport plus the track's voices in it.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct NormBar {
    /// Zero-based bar index.
    pub index: usize,
    /// Meter as `[numerator, denominator]`.
    pub time_sig: [u8; 2],
    /// Tempo in BPM at the bar start.
    pub tempo: f64,
    /// Absolute start tick (inclusive).
    pub start_tick: u32,
    /// Absolute end tick (exclusive).
    pub end_tick: u32,
    /// Voices with at least one note in this bar.
    pub voices: Vec<NormVoice>,
}

/// A normalized voice: its id and the notes it plays in the enclosing bar.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NormVoice {
    /// Voice identifier; 0 is the primary voice.
    pub id: u8,
    /// Notes, sorted by `(onset, pitch, string, fret)`.
    pub notes: Vec<NormNote>,
}

/// A normalized note — the comparison-relevant fields only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NormNote {
    /// Absolute onset tick.
    pub onset: u32,
    /// Duration in ticks.
    pub dur: u32,
    /// MIDI pitch (0–127).
    pub pitch: u8,
    /// MIDI velocity (0–127).
    pub velocity: u8,
    /// 1-indexed string number, when a fretboard position is present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub string: Option<u8>,
    /// Fret number, when a fretboard position is present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fret: Option<u8>,
    /// Per-note technique marks, in declaration order (omitted when empty).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub marks: Vec<String>,
    /// Spanning techniques covering this onset, sorted (omitted when empty).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub spans: Vec<String>,
}

/// Projects a [`Score`] into its [`NormalizedScore`] (ADR-0020).
#[must_use]
pub fn normalize(_score: &Score) -> NormalizedScore {
    unimplemented!("dump::normalize — green step")
}
