//! Deriving chord-quality evidence from a track's notation (#75).
//!
//! The harmony tags (`maj7`/`min7`/`sus2`/`add9`/`power_chord`) name chord
//! *voicings*. Guitar Pro records a chord as simultaneous notes in one
//! [`EventGroup`](crate::score::EventGroup) of kind
//! [`Chord`](crate::score::EventGroupKind::Chord); rather than ask a curator to
//! name what the tab already spells out, [`derive_harmony`] reads each chord's
//! pitch-class set and matches it against the voicing templates.
//!
//! Like [`derive_techniques`](crate::technique::derive_techniques) it is
//! **presence-only** and template-exact — a pure function of the score with no
//! thresholds (SPEC §6). Inversions are tagged by their quality (the voicing is
//! present whichever tone is in the bass); dedicated **slash-chord** detection is
//! a deliberate follow-up (see `docs/decisions.log.md`), because the common
//! plain-triad slash (e.g. `G/B`) is not expressible with the seventh/sus/add
//! templates and would need its own bass-vs-root model.

use std::collections::HashSet;

use crate::corpus::SwancoreTag;
use crate::event::Pitch;
use crate::score::{AtomEvent, EventGroup, EventGroupKind, Score};

/// Chord-quality tags in canonical (taxonomy-declaration) order.
const HARMONY_TAGS: [SwancoreTag; 5] = [
    SwancoreTag::Maj7,
    SwancoreTag::Min7,
    SwancoreTag::Sus2,
    SwancoreTag::Add9,
    SwancoreTag::PowerChord,
];

/// Root-anchored pitch-class templates (root = 0) for the seventh/sus/add
/// voicings, each sorted ascending. Power chords are matched separately as a
/// bare-fifth dyad, so they are not listed here.
const TEMPLATES: [(&[u8], SwancoreTag); 4] = [
    (&[0, 4, 7, 11], SwancoreTag::Maj7),
    (&[0, 3, 7, 10], SwancoreTag::Min7),
    (&[0, 2, 7], SwancoreTag::Sus2),
    (&[0, 2, 4, 7], SwancoreTag::Add9),
];

/// Derives the chord-quality tags present in `track_index`.
///
/// Scans every voice's chord-bearing groups, classifies each chord's
/// pitch-class set, and returns the matched [`SwancoreTag`]s in canonical order,
/// deduplicated. Empty when the track index is out of range or nothing matches.
#[must_use]
pub fn derive_harmony(score: &Score, track_index: usize) -> Vec<SwancoreTag> {
    let Some(track) = score.tracks.get(track_index) else {
        return Vec::new();
    };

    // Every voice — some importers split one track into several voices, so a
    // chord in a secondary voice must still be seen (cf. the technique deriver).
    let mut found: HashSet<SwancoreTag> = HashSet::new();
    for voice in &track.voices {
        for group in &voice.event_groups {
            if let Some(tag) = classify_group(group) {
                found.insert(tag);
            }
        }
    }

    HARMONY_TAGS
        .into_iter()
        .filter(|tag| found.contains(tag))
        .collect()
}

/// Classifies the chord borne by a group, if it is a chord-bearing group with a
/// recognisable voicing.
fn classify_group(group: &EventGroup) -> Option<SwancoreTag> {
    // Only genuinely simultaneous groups are chords; melody (`Single`) and
    // rhythmic/ornamental groupings are not. Arpeggios and chords split across
    // voices are a deliberate follow-up.
    if !matches!(group.kind, EventGroupKind::Chord | EventGroupKind::Strum) {
        return None;
    }
    classify(&pitch_classes(group))
}

/// The distinct pitch classes (0–11) sounding in a group, sorted ascending.
fn pitch_classes(group: &EventGroup) -> Vec<u8> {
    let mut classes: Vec<u8> = group
        .atoms
        .iter()
        .filter_map(|atom| match atom {
            AtomEvent::Note(note) => Some(pitch_class(note.pitch)),
            AtomEvent::Rest(_) => None,
        })
        .collect();
    classes.sort_unstable();
    classes.dedup();
    classes
}

/// Matches a sorted, distinct pitch-class set against the voicing templates.
fn classify(classes: &[u8]) -> Option<SwancoreTag> {
    // A power chord is the bare perfect-fifth dyad (root + fifth, any octave):
    // two distinct classes a fifth (7) — or its inversion, a fourth (5) — apart.
    if let &[low, high] = classes {
        let interval = high.wrapping_sub(low);
        return (interval == 5 || interval == 7).then_some(SwancoreTag::PowerChord);
    }
    // Otherwise try each chord tone as the root — the root of a maj7/min7/sus2/
    // add9 is always a chord tone — so inversions match their quality too.
    classes
        .iter()
        .find_map(|&root| match_templates(classes, root))
}

/// The template tag whose interval set, rooted at `root`, equals `classes`
/// reduced to root-relative intervals.
fn match_templates(classes: &[u8], root: u8) -> Option<SwancoreTag> {
    let mut intervals: Vec<u8> = classes.iter().map(|&pc| interval_above(root, pc)).collect();
    intervals.sort_unstable();
    TEMPLATES
        .into_iter()
        .find_map(|(template, tag)| (intervals.as_slice() == template).then_some(tag))
}

/// The pitch class `pc` expressed as an interval above `root`, in `0..12`. Both
/// arguments are pitch classes in `0..12`, so `pc + 12 - root` lies in `1..24`
/// and the reduction never underflows.
fn interval_above(root: u8, pc: u8) -> u8 {
    pc.wrapping_add(12)
        .wrapping_sub(root)
        .checked_rem(12)
        .unwrap_or(0)
}

/// The pitch class (0–11) of a MIDI pitch.
fn pitch_class(pitch: Pitch) -> u8 {
    pitch.0.checked_rem(12).unwrap_or(0)
}
