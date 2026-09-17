//! Content fingerprints: SHA-256 over a domain-tagged, length-prefixed walk of
//! the canonical model.
//!
//! A fingerprint is defined over the *model*, not over any serialisation of it,
//! so a later persisted form must reproduce these values rather than define its
//! own. Every walk destructures its type exhaustively: a field added to the
//! model is a compile error here, never a silently unhashed fact. Floats are
//! hashed by their bits; strings and sequences carry their length, so no two
//! distinct values share an encoding.

use std::fmt;
use std::fmt::Write as _;

use griff_core::event::{
    ConfidenceBps, FretboardPosition, NoteMark, NotePosition, SpanTechnique, TechniqueEvidence,
    TechniqueSource, TimeSignature,
};
use griff_core::generate::{PitchMaterial, RhythmTemplate, TemplateNote};
use griff_core::generation_input::GenerationAsk;
use griff_core::gesture::GestureControl;
use griff_core::score::{
    AtomEvent, AtomNote, AtomRest, EventGroup, EventGroupKind, ImportWarning, LossReport,
    MasterBar, RepeatMarker, Score, SourceMeta, TechniqueSpan, Track, Voice,
};
use griff_core::slice::TickRange;
use sha2::{Digest, Sha256};

/// A 32-byte content fingerprint.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Fingerprint(pub [u8; 32]);

impl Fingerprint {
    /// Lowercase hex, 64 characters.
    #[must_use]
    pub fn to_hex(&self) -> String {
        self.0.iter().fold(String::with_capacity(64), |mut acc, b| {
            write!(acc, "{b:02x}").ok();
            acc
        })
    }
}

impl fmt::Debug for Fingerprint {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Fingerprint({})", self.to_hex())
    }
}

/// An incremental, domain-tagged fingerprint builder.
pub(crate) struct Hasher(Sha256);

impl Hasher {
    /// A builder whose every output is separated from other domains' by `domain`.
    pub(crate) fn new(domain: &str) -> Self {
        let mut hasher = Self(Sha256::new());
        hasher.str(domain);
        hasher
    }

    pub(crate) fn u8(&mut self, v: u8) {
        self.0.update([v]);
    }

    pub(crate) fn u16(&mut self, v: u16) {
        self.0.update(v.to_le_bytes());
    }

    pub(crate) fn u32(&mut self, v: u32) {
        self.0.update(v.to_le_bytes());
    }

    pub(crate) fn u64(&mut self, v: u64) {
        self.0.update(v.to_le_bytes());
    }

    pub(crate) fn usize(&mut self, v: usize) {
        self.u64(u64::try_from(v).unwrap_or(u64::MAX));
    }

    pub(crate) fn bool(&mut self, v: bool) {
        self.u8(u8::from(v));
    }

    pub(crate) fn f64(&mut self, v: f64) {
        self.u64(v.to_bits());
    }

    pub(crate) fn str(&mut self, s: &str) {
        self.usize(s.len());
        self.0.update(s.as_bytes());
    }

    pub(crate) fn fingerprint(&mut self, fp: Fingerprint) {
        self.0.update(fp.0);
    }

    pub(crate) fn option_fingerprint(&mut self, fp: Option<Fingerprint>) {
        match fp {
            None => self.u8(0),
            Some(fp) => {
                self.u8(1);
                self.fingerprint(fp);
            }
        }
    }

    pub(crate) fn finish(self) -> Fingerprint {
        Fingerprint(self.0.finalize().into())
    }
}

/// The fingerprint of a whole score: every bar, track, voice, group, atom,
/// technique span, tuning, source metadata and loss warning.
#[must_use]
pub fn score_fingerprint(score: &Score) -> Fingerprint {
    let mut h = Hasher::new("griff.score.v1");
    let Score {
        ticks_per_quarter,
        master_bars,
        tracks,
        source_meta,
        loss,
    } = score;
    h.u16(*ticks_per_quarter);
    h.usize(master_bars.len());
    for bar in master_bars {
        master_bar(&mut h, bar);
    }
    h.usize(tracks.len());
    for t in tracks {
        track(&mut h, t);
    }
    match source_meta {
        None => h.u8(0),
        Some(SourceMeta { format }) => {
            h.u8(1);
            option_str(&mut h, format.as_deref());
        }
    }
    let LossReport { warnings } = loss;
    h.usize(warnings.len());
    for warning in warnings {
        import_warning(&mut h, warning);
    }
    h.finish()
}

/// The fingerprint of an ordered rhythm-template palette — order is behaviour,
/// since the generator rotates templates in palette order.
#[must_use]
pub fn rhythms_fingerprint(rhythms: &[RhythmTemplate]) -> Fingerprint {
    let mut h = Hasher::new("griff.rhythms.v1");
    h.usize(rhythms.len());
    for RhythmTemplate { notes } in rhythms {
        h.usize(notes.len());
        for TemplateNote { offset, duration } in notes {
            h.u32(offset.0);
            h.u32(duration.0);
        }
    }
    h.finish()
}

/// The fingerprint of an ordered novelty reference set.
#[must_use]
pub fn references_fingerprint(references: &[Score]) -> Fingerprint {
    let mut h = Hasher::new("griff.references.v1");
    h.usize(references.len());
    for reference in references {
        h.fingerprint(score_fingerprint(reference));
    }
    h.finish()
}

/// The fingerprint of a gesture channel, `None` included.
#[must_use]
pub fn gesture_fingerprint(gesture: Option<GestureControl>) -> Fingerprint {
    let mut h = Hasher::new("griff.gesture.v1");
    match gesture {
        None => h.u8(0),
        Some(GestureControl {
            burst_notes,
            rest_quarters,
        }) => {
            h.u8(1);
            h.usize(burst_notes);
            h.f64(rest_quarters);
        }
    }
    h.finish()
}

/// The fingerprint of an ask: seed, bars, variants per strategy, the gesture
/// request, and the carried tonal context.
#[must_use]
pub fn ask_fingerprint(ask: &GenerationAsk) -> Fingerprint {
    let mut h = Hasher::new("griff.ask.v1");
    let GenerationAsk {
        seed,
        bars,
        variants_per_strategy,
        gesture,
        tonal,
    } = ask;
    h.u64(*seed);
    h.usize(*bars);
    h.usize(*variants_per_strategy);
    h.bool(*gesture);
    // The tonal context's public wire form is its serde projection: the one
    // representation `griff_core::tonal` itself promises to keep stable.
    let tonal = tonal.and_then(|t| serde_json::to_string(&t).ok());
    option_str(&mut h, tonal.as_deref());
    h.finish()
}

/// Writes a scale: its root and ordered intervals.
pub(crate) fn pitch_material(h: &mut Hasher, material: &PitchMaterial) {
    let PitchMaterial { root, intervals } = material;
    h.u8(root.0);
    h.usize(intervals.len());
    for &interval in intervals {
        h.u8(interval);
    }
}

fn option_str(h: &mut Hasher, s: Option<&str>) {
    match s {
        None => h.u8(0),
        Some(s) => {
            h.u8(1);
            h.str(s);
        }
    }
}

fn tick_range(h: &mut Hasher, range: TickRange) {
    let TickRange { start, end } = range;
    h.u32(start.0);
    h.u32(end.0);
}

fn master_bar(h: &mut Hasher, bar: &MasterBar) {
    let MasterBar {
        index,
        tick_range: range,
        time_signature,
        tempo,
        repeat,
    } = bar;
    h.u64(*index);
    tick_range(h, *range);
    let TimeSignature {
        numerator,
        denominator,
    } = time_signature;
    h.u8(*numerator);
    h.u8(*denominator);
    h.u32(tempo.bpm_numerator());
    h.u32(tempo.bpm_denominator());
    let RepeatMarker { start, play_count } = repeat;
    h.bool(*start);
    h.u8(*play_count);
}

fn track(h: &mut Hasher, track: &Track) {
    let Track {
        name,
        channel,
        voices,
        tuning,
    } = track;
    option_str(h, name.as_deref());
    h.u8(*channel);
    let strings = tuning.open_strings();
    h.usize(strings.len());
    for pitch in strings {
        h.u8(pitch.0);
    }
    h.usize(voices.len());
    for Voice { id, event_groups } in voices {
        h.u8(*id);
        h.usize(event_groups.len());
        for group in event_groups {
            event_group(h, group);
        }
    }
}

fn event_group(h: &mut Hasher, group: &EventGroup) {
    let EventGroup {
        kind,
        atoms,
        technique_spans,
    } = group;
    match *kind {
        EventGroupKind::Single => h.u8(0),
        EventGroupKind::Chord => h.u8(1),
        EventGroupKind::Arpeggio => h.u8(2),
        EventGroupKind::Strum => h.u8(3),
        EventGroupKind::Tuplet { num, den } => {
            h.u8(4);
            h.u8(num);
            h.u8(den);
        }
        EventGroupKind::Grace => h.u8(5),
    }
    h.usize(atoms.len());
    for atom in atoms {
        match *atom {
            AtomEvent::Note(AtomNote {
                absolute_start,
                duration,
                pitch,
                velocity,
                marks,
                position,
            }) => {
                h.u8(0);
                h.u32(absolute_start.0);
                h.u32(duration.0);
                h.u8(pitch.0);
                h.u8(velocity.0);
                for mark in NoteMark::ALL {
                    h.bool(marks.contains(mark));
                }
                match position {
                    None => h.u8(0),
                    Some(NotePosition {
                        position: FretboardPosition { string, fret },
                        evidence,
                    }) => {
                        h.u8(1);
                        h.u8(string);
                        h.u8(fret);
                        technique_evidence(h, evidence);
                    }
                }
            }
            AtomEvent::Rest(AtomRest {
                absolute_start,
                duration,
            }) => {
                h.u8(1);
                h.u32(absolute_start.0);
                h.u32(duration.0);
            }
        }
    }
    h.usize(technique_spans.len());
    for &TechniqueSpan {
        technique,
        tick_range: range,
        evidence,
    } in technique_spans
    {
        h.u8(match technique {
            SpanTechnique::Slide => 0,
            SpanTechnique::Bend => 1,
            SpanTechnique::Legato => 2,
            SpanTechnique::PalmMute => 3,
            SpanTechnique::HammerOn => 4,
            SpanTechnique::PullOff => 5,
            SpanTechnique::Vibrato => 6,
            SpanTechnique::LetRing => 7,
        });
        tick_range(h, range);
        technique_evidence(h, evidence);
    }
}

fn technique_evidence(h: &mut Hasher, evidence: TechniqueEvidence) {
    let TechniqueEvidence { source, confidence } = evidence;
    h.u8(match source {
        TechniqueSource::Explicit => 0,
        TechniqueSource::InferredFromMidi => 1,
    });
    h.u16(ConfidenceBps::get(confidence));
}

fn import_warning(h: &mut Hasher, warning: &ImportWarning) {
    match warning {
        ImportWarning::TrackNameInvalidUtf8 { track_index } => {
            h.u8(0);
            h.u64(*track_index);
        }
        ImportWarning::SmpteTimingUnsupported => h.u8(1),
        ImportWarning::TempoApproximated {
            bar_index,
            nearest_micros,
        } => {
            h.u8(2);
            h.u64(*bar_index);
            h.u32(*nearest_micros);
        }
        ImportWarning::Other(message) => {
            h.u8(3);
            h.str(message);
        }
    }
}
