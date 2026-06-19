//! Deriving guitar-technique evidence from a track's notation (ADR-0018).
//!
//! Guitar Pro import records techniques explicitly: spanning techniques (slide,
//! bend, legato, palm-mute, hammer-on, pull-off, vibrato) land on
//! [`TechniqueSpan`](crate::score::TechniqueSpan)s, and per-note ones (accent,
//! ghost, staccato, dead-note, natural/pinch harmonic, tap) on
//! [`NoteMarks`](crate::event::NoteMarks). Curation reads that for free instead
//! of asking a human to re-tag what the tab already states.
//!
//! [`derive_techniques`] scans one track and returns both the [`SwancoreTag`]s
//! that have a *direct, per-occurrence* tag and the full free-form
//! technique-name list for `ChunkMeta::techniques`. It is presence-only — no
//! thresholds or heuristics — so it stays a pure function of the score (SPEC
//! §6). The passage-level tags (`TappingPassage`, `LegatoPassage`) are about
//! dominance, not presence, so they are deliberately *not* derived here.

use std::collections::BTreeSet;

use crate::corpus::SwancoreTag;
use crate::event::{NoteMark, SpanTechnique};
use crate::score::{AtomEvent, Score};

/// Techniques derived from one track's notation.
///
/// `tags` are the [`SwancoreTag`]s with a dedicated per-occurrence variant;
/// `names` is the superset of `lower_snake_case` technique names (it also
/// records legato, accent, ghost, staccato, dead-note and tap, which have no
/// dedicated tag). Both are in a stable canonical order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DerivedTechniques {
    /// Directly-taggable techniques present in the track.
    pub tags: Vec<SwancoreTag>,
    /// Names of every technique present — a superset of `tags`.
    pub names: Vec<String>,
}

/// Spanning techniques in canonical (declaration) order.
const SPANS: [SpanTechnique; 8] = [
    SpanTechnique::Slide,
    SpanTechnique::Bend,
    SpanTechnique::Legato,
    SpanTechnique::PalmMute,
    SpanTechnique::HammerOn,
    SpanTechnique::PullOff,
    SpanTechnique::Vibrato,
    SpanTechnique::LetRing,
];

/// `lower_snake_case` name for a spanning technique.
const fn span_name(t: SpanTechnique) -> &'static str {
    match t {
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

/// The dedicated tag for a spanning technique, if one exists. Legato has only
/// the passage-level `LegatoPassage`, which needs dominance — not derived here.
const fn span_tag(t: SpanTechnique) -> Option<SwancoreTag> {
    match t {
        SpanTechnique::Slide => Some(SwancoreTag::Slide),
        SpanTechnique::Bend => Some(SwancoreTag::Bend),
        SpanTechnique::PalmMute => Some(SwancoreTag::PalmMute),
        SpanTechnique::HammerOn => Some(SwancoreTag::HammerOn),
        SpanTechnique::PullOff => Some(SwancoreTag::PullOff),
        SpanTechnique::Vibrato => Some(SwancoreTag::Vibrato),
        SpanTechnique::LetRing => Some(SwancoreTag::LetRing),
        SpanTechnique::Legato => None,
    }
}

/// `lower_snake_case` name for a per-note mark.
const fn mark_name(m: NoteMark) -> &'static str {
    match m {
        NoteMark::Accent => "accent",
        NoteMark::Ghost => "ghost",
        NoteMark::Staccato => "staccato",
        NoteMark::DeadNote => "dead_note",
        NoteMark::HarmonicNatural => "natural_harmonic",
        NoteMark::HarmonicPinch => "pinch_harmonic",
        NoteMark::Tap => "tap",
    }
}

/// The dedicated tag for a per-note mark, if one exists. Only the two harmonics
/// have one; accent/ghost/staccato/dead-note/tap are names-only.
const fn mark_tag(m: NoteMark) -> Option<SwancoreTag> {
    match m {
        NoteMark::HarmonicNatural => Some(SwancoreTag::NaturalHarmonic),
        NoteMark::HarmonicPinch => Some(SwancoreTag::ArtificialHarmonic),
        NoteMark::Accent
        | NoteMark::Ghost
        | NoteMark::Staccato
        | NoteMark::DeadNote
        | NoteMark::Tap => None,
    }
}

/// Derives the techniques present in `track_index`.
///
/// Scans every voice, matching `track_notes`/`technique_share`, which measure a
/// track as a whole (some importers split one track into several voices) — so
/// the `techniques` metadata can't disagree with the technical metric. Empty
/// when the index is out of range.
#[must_use]
pub fn derive_techniques(score: &Score, track_index: usize) -> DerivedTechniques {
    let Some(track) = score.tracks.get(track_index) else {
        return DerivedTechniques::default();
    };

    // Every voice — like track_notes/technique_share, which measure a track as a
    // whole (some importers split one track into several voices).
    let mut spans: BTreeSet<SpanTechnique> = BTreeSet::new();
    let mut marks: BTreeSet<NoteMark> = BTreeSet::new();
    for voice in &track.voices {
        for group in &voice.event_groups {
            for span in &group.technique_spans {
                spans.insert(span.technique);
            }
            for atom in &group.atoms {
                if let AtomEvent::Note(note) = atom {
                    for mark in note.marks.iter() {
                        marks.insert(mark);
                    }
                }
            }
        }
    }

    let mut tags = Vec::new();
    let mut names = Vec::new();
    for t in SPANS {
        if spans.contains(&t) {
            names.push(span_name(t).to_owned());
            if let Some(tag) = span_tag(t) {
                tags.push(tag);
            }
        }
    }
    for m in NoteMark::ALL {
        if marks.contains(&m) {
            names.push(mark_name(m).to_owned());
            if let Some(tag) = mark_tag(m) {
                tags.push(tag);
            }
        }
    }
    DerivedTechniques { tags, names }
}

/// Unions curator-chosen tags with derived ones.
///
/// The chosen order is kept, then any derived tag not already present is
/// appended — so auto-derivation *adds* technique tags without overriding or
/// reordering the curator's choices. Stable and idempotent.
#[must_use]
pub fn merge_tags(chosen: &[SwancoreTag], derived: &[SwancoreTag]) -> Vec<SwancoreTag> {
    let mut out = chosen.to_vec();
    for &tag in derived {
        if !out.contains(&tag) {
            out.push(tag);
        }
    }
    out
}
