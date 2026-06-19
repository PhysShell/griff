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
//! sorted by `(onset, string, fret, pitch)` — the `(bar, voice, onset, string)`
//! order ADR-0020 mandates, with `fret`/`pitch` as final tie-breaks for a total
//! order — so group structure and import ordering do not affect the output.
//! Empty voices are dropped; transport-only bars stay (their meter/tempo is
//! meaningful).

use serde::Serialize;

use crate::{
    event::{NoteMark, SpanTechnique},
    score::{AtomEvent, AtomNote, ImportWarning, MasterBar, Score, TechniqueSpan, Track, Voice},
};

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
    /// Notes, sorted by `(onset, string, fret, pitch)` (ADR-0020).
    pub notes: Vec<NormNote>,
}

/// A normalized note — the comparison-relevant fields only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NormNote {
    /// Absolute onset tick.
    pub onset_tick: u32,
    /// Duration in ticks.
    pub dur_tick: u32,
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
pub fn normalize(score: &Score) -> NormalizedScore {
    let tracks = score
        .tracks
        .iter()
        .map(|track| NormTrack {
            name: track.name.clone(),
            channel: track.channel,
            tuning: track.tuning.open_strings().iter().map(|p| p.0).collect(),
            bars: score
                .master_bars
                .iter()
                .map(|bar| norm_bar(track, bar))
                .collect(),
        })
        .collect();
    let loss = score.loss.warnings.iter().map(warning_label).collect();
    NormalizedScore {
        ppqn: score.ticks_per_quarter,
        loss,
        tracks,
    }
}

/// Projects one track's content within one master bar.
fn norm_bar(track: &Track, bar: &MasterBar) -> NormBar {
    let mut voices: Vec<NormVoice> = track
        .voices
        .iter()
        .filter_map(|voice| {
            let notes = voice_notes_in_bar(voice, bar);
            (!notes.is_empty()).then_some(NormVoice {
                id: voice.id,
                notes,
            })
        })
        .collect();
    // Canonical order independent of `track.voices` import order (ADR-0020).
    voices.sort_by_key(|voice| voice.id);
    NormBar {
        index: bar.index,
        time_sig: [bar.time_signature.numerator, bar.time_signature.denominator],
        tempo: bar.tempo.0,
        start_tick: bar.tick_range.start.0,
        end_tick: bar.tick_range.end.0,
        voices,
    }
}

/// Collects and canonically sorts a voice's notes whose onset falls in `bar`.
fn voice_notes_in_bar(voice: &Voice, bar: &MasterBar) -> Vec<NormNote> {
    let spans: Vec<&TechniqueSpan> = voice
        .event_groups
        .iter()
        .flat_map(|group| group.technique_spans.iter())
        .collect();

    let mut notes: Vec<NormNote> = voice
        .event_groups
        .iter()
        .flat_map(|group| group.atoms.iter())
        .filter_map(|atom| match atom {
            AtomEvent::Note(note) => {
                let onset = note.absolute_start.0;
                (onset >= bar.tick_range.start.0 && onset < bar.tick_range.end.0)
                    .then(|| norm_note(note, &spans))
            }
            AtomEvent::Rest(_) => None,
        })
        .collect();

    notes.sort_by_key(|note| {
        (
            note.onset_tick,
            note.string.unwrap_or(0),
            note.fret.unwrap_or(0),
            note.pitch,
        )
    });
    notes
}

/// Projects one note, attaching the spans whose range covers its onset.
fn norm_note(note: &AtomNote, spans: &[&TechniqueSpan]) -> NormNote {
    let (string, fret) = note.position.map_or((None, None), |pos| {
        (Some(pos.position.string), Some(pos.position.fret))
    });
    let onset = note.absolute_start.0;
    let mut span_names: Vec<String> = spans
        .iter()
        .filter(|span| onset >= span.tick_range.start.0 && onset < span.tick_range.end.0)
        .map(|span| span_name(span.technique).to_owned())
        .collect();
    span_names.sort();
    span_names.dedup();
    NormNote {
        onset_tick: onset,
        dur_tick: note.duration.0,
        pitch: note.pitch.0,
        velocity: note.velocity.0,
        string,
        fret,
        marks: note.marks.iter().map(|m| mark_name(m).to_owned()).collect(),
        spans: span_names,
    }
}

/// Stable wire label for a per-note mark.
const fn mark_name(mark: NoteMark) -> &'static str {
    match mark {
        NoteMark::Accent => "accent",
        NoteMark::Ghost => "ghost",
        NoteMark::Staccato => "staccato",
        NoteMark::DeadNote => "dead_note",
        NoteMark::HarmonicNatural => "harmonic_natural",
        NoteMark::HarmonicPinch => "harmonic_pinch",
        NoteMark::Tap => "tap",
    }
}

/// Stable wire label for a spanning technique.
const fn span_name(technique: SpanTechnique) -> &'static str {
    match technique {
        SpanTechnique::Slide => "slide",
        SpanTechnique::Bend => "bend",
        SpanTechnique::Legato => "legato",
        SpanTechnique::PalmMute => "palm_mute",
        SpanTechnique::HammerOn => "hammer_on",
        SpanTechnique::PullOff => "pull_off",
        SpanTechnique::Vibrato => "vibrato",
        SpanTechnique::LetRing => "let_ring",
    }
}

/// Stable wire label for an import warning.
fn warning_label(warning: &ImportWarning) -> String {
    match warning {
        ImportWarning::TrackNameInvalidUtf8 { track_index } => {
            format!("track_name_invalid_utf8:{track_index}")
        }
        ImportWarning::SmpteTimingUnsupported => "smpte_timing_unsupported".to_owned(),
        ImportWarning::Other(message) => format!("other:{message}"),
    }
}
