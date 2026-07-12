//! `griff-cli` internal library — the reusable seam shared by the `griff`
//! binary and experimental A/B harnesses (see [`generation_input`]).
//!
//! This is **not** a stable public API: it exists so tooling reuses the exact
//! production corpus→generation compiler instead of reimplementing (and
//! drifting from) it. Everything here is `#[doc(hidden)]` and stability-exempt.

#![doc(hidden)]

use griff_core::score::{AtomEvent, Track};

pub mod generation_input;

/// Notes in a track's *primary* (first) voice — the track-selection predicate
/// shared by curation, splitting, and corpus loading, so selection and
/// measurement agree on which track sounds.
#[must_use]
pub fn primary_voice_note_count(track: &Track) -> usize {
    track.voices.first().map_or(0, |v| {
        v.event_groups
            .iter()
            .flat_map(|g| &g.atoms)
            .filter(|a| matches!(a, AtomEvent::Note(_)))
            .count()
    })
}
